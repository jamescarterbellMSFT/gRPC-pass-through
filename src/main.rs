use std::future::Future;
use corp::vault_client::VaultClient;
use tonic::transport::channel::Channel;
use crossbeam::channel::{bounded, Sender, Receiver};
use serde::Serialize;
use std::pin::Pin;
use warp::{Filter, Rejection};


pub mod corp{
    tonic::include_proto!("corp");
}

 
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>  {
    //let f = FilterBuilder::build(warp::put(), "/deposit", VaultClient::deposit);
    let pool = ClientPool::build_n_with_async(12, || async {VaultClient::connect("127.0.0.1:8081").await.unwrap()}).await;
    let f = warp::put()
                .and(warp::path("/deposit"))
                .and(warp::body::json())
                .and_then(|q: corp::DepositRequest| async {
                    let mut client = pool.get_client_async().await;
                    Ok(client.deposit(q).await.unwrap())
                });
    Ok(())
}

pub struct ClientPool<T>{
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> ClientPool<T>
    where T: 'static + Send{
    pub fn build_n_with(n: usize, builder: fn() -> T) -> Self{
        let (sender, receiver) = bounded(n);
        
        let pool = Self{
            sender,
            receiver
        };

        for _ in 0..n{
            pool.sender.send((builder)()).unwrap();
        };

        pool
    }
    
    pub async fn build_n_with_async<Q>(n: usize, builder: fn() -> Q) -> Self
        where Q: Future<Output = T>{
        let (sender, receiver) = bounded(n);
        
        let pool = Self{
            sender,
            receiver
        };

        for _ in 0..n{
            pool.sender.send((builder)().await).unwrap();
        };

        pool
    }

    pub fn get_client<'a>(&'a self) -> ClientGuard<'a, T>{
        ClientGuard{
            source: self,
            client: Some(self.receiver.recv().unwrap())
        }
    }

    pub async fn get_client_async<'a>(&'a self) -> ClientGuard<'a, T>{
        let mut recv = self.receiver.clone();
        ClientGuard{
            source: self,
            client: tokio::task::spawn_blocking(move ||{
                Some(recv.recv().unwrap())
            }).await.unwrap(),
        }
    }
}

pub struct ClientGuard<'a, T>{
    source: &'a ClientPool<T>,
    client: Option<T>,
}

impl<'a, T> std::ops::Drop for ClientGuard<'a, T>{
    fn drop(&mut self){
        self.source.sender.send(
            self.client.take().unwrap()
        );
    }
}

impl<'a, T> std::ops::Deref for ClientGuard<'a, T>{
    type Target = T;

    fn deref(&self) -> &Self::Target{
        self.client.as_ref().unwrap()
    }
}

impl<'a, T> std::ops::DerefMut for ClientGuard<'a, T>{
    fn deref_mut(&mut self) -> &mut Self::Target{
        self.client.as_mut().unwrap()
    }
}

/*
struct FilterBuilder;

impl FilterBuilder{
    fn build<C, M, R>(method: impl Filter, path: impl AsRef<str>, grpc_fn: impl Fn(&mut C, M) -> Future<Output = R>) -> warp::AndThen{
        let f = method
            .and(warp::path(path))
            .and_then();
        Box::new(f)
    }
}
*/