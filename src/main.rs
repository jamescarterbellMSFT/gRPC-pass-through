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
    let pool = ClientPool::build_n_with_async(12, || async {VaultClient::connect("127.0.0.1:8081").await.unwrap()}).await;
    let f = Box::new(|s: String| -> Pin<Box<dyn Future<Output = Result<warp::reply::Json, Rejection>>>> { Box::pin(async move {
            let mut client = pool.get_client_async().await;
            let req = serde_json::from_str::<corp::DepositRequest>(&s).unwrap();
            let reply = &client.deposit(req).await.unwrap().into_inner();
            Ok(warp::reply::json(reply))
    })});
    let f = warp::put()
                .and(warp::path("/deposit"))
                .and(warp::body::json())
                .and_then(f);
    Ok(())
}

async fn test(word: String) -> Result<impl warp::Reply, std::convert::Infallible>{
    Ok("Hi")
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
        let recv = self.receiver.clone();
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

