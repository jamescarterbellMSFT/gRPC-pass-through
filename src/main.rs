#![feature(proc_macro_hygiene, decl_macro, async_closure)]

#[macro_use] extern crate rocket;

use std::future::Future;
use rocket::State;
use rocket_contrib::json::Json;
use corp::vault_client::VaultClient;
use tonic::transport::channel::Channel;
use crossbeam::channel::{bounded, Sender, Receiver};

pub mod corp{
    tonic::include_proto!("corp");
}
 
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>  {
    rocket::ignite()
        .mount("/", routes![deposit, withdraw])
        .manage(
            ClientPool::build_n_with_async(
                24, async || VaultClient::connect("https://localhost:8081").await.unwrap()
            ).await
        )
        .launch()
        .await?;

    Ok(())
}

pub struct ClientPool<T>{
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> ClientPool<T>{
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

#[put("/deposit", data = "<req>")]
async fn deposit(req: Json<corp::DepositRequest>, pool: State<'_, ClientPool<VaultClient<Channel>>>) -> Result<Json<corp::AccountReply>, String>{
    let mut grpc_client = pool.get_client();
    match grpc_client.deposit(req.into_inner()).await{
        Ok(res) => Ok(Json(res.into_inner())),
        Err(e) => Err(e.to_string()),
    }
}

#[put("/withdraw", data = "<req>")]
async fn withdraw(req: Json<corp::WithdrawRequest>, pool: State<'_, ClientPool<VaultClient<Channel>>>) -> Result<Json<corp::AccountReply>, String>{
    let mut grpc_client = pool.get_client();
    match grpc_client.withdraw(req.into_inner()).await{
        Ok(res) => Ok(Json(res.into_inner())),
        Err(e) => Err(e.to_string()),
    }
}