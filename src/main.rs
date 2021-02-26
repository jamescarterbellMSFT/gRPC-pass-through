#![feature(proc_macro_hygiene, decl_macro, async_closure)]

#[macro_use] extern crate rocket;

use std::future::Future;
use rocket::{Route, Request, Data, State, handler::{Outcome, Handler}, data::{FromData}, http::Method};
use rocket_contrib::json::Json;
use corp::vault_client::VaultClient;
use tonic::transport::channel::Channel;
use crossbeam::channel::{bounded, Sender, Receiver};
use serde::Serialize;
use std::pin::Pin;


pub mod corp{
    tonic::include_proto!("corp");
}

 
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>  {
    let route = GrpcHandlerBuilder::<VaultClient<Channel>>::build_route(Method::Put, "/deposit", |c, m| Box::pin(VaultClient::deposit(&mut c, m)));
    rocket::ignite()
        .mount("/", routes![withdraw])
        //.mount("/", vec![(builder)])
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
        let client = tokio::task::spawn_blocking(move || recv.recv().unwrap()).await.unwrap();
        ClientGuard{
            source: self,
            client: Some(client)
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
    match VaultClient::withdraw(&mut grpc_client, req.into_inner()).await{
        Ok(res) => Ok(Json(res.into_inner())),
        Err(e) => Err(e.to_string()),
    }
}


struct GrpcHandlerBuilder<C>{
    c: std::marker::PhantomData<C>,
}

impl<C> GrpcHandlerBuilder<C>
    where C: 'static + Sync + Send{

    fn build_handler<F, M, R>(grpc_fn: F) -> GrpcHandler<C, M, R, F>
        where F: 'static + Send + Sync + Copy + Fn(&mut C, M) -> Pin<Box<dyn Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send>>,
            M: 'static + Sync + Send + FromData,
            R: 'static + Sync + Send + Serialize,{
        GrpcHandler{
            f: Box::pin(grpc_fn),
            c: std::marker::PhantomData,
            m: std::marker::PhantomData,
            r: std::marker::PhantomData,
        }
    }

    fn build_route<S, F, M, R>(method: Method, path: S, grpc_fn: F) -> Route
        where F: 'static + Send + Sync + Copy + Fn(&mut C, M) -> Pin<Box<dyn Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send>>,
            M: 'static + Sync + Send + FromData,
            R: 'static + Sync + Send + Serialize,
            S: AsRef<str>{
        let handler = Self::build_handler::<F, M, R>(grpc_fn);
        Route::new(method, path, handler)
    }
}


struct GrpcHandler<C, M, R, F>
    where F: Copy{
    f: Pin<Box<F>>,
    c: std::marker::PhantomData<C>,
    m: std::marker::PhantomData<M>,
    r: std::marker::PhantomData<R>,
}

impl<C, M, R, F> Clone for GrpcHandler<C, M, R, F>
    where F: Copy{

    fn clone(&self) -> Self{
        Self{
            f: Box::pin(*self.f),
            c: std::marker::PhantomData,
            m: std::marker::PhantomData,
            r: std::marker::PhantomData
        }
    }
}


#[rocket::async_trait]
impl<C, M, R, F> Handler for GrpcHandler<C, M, R, F>
    where F: 'static + Send + Sync + Copy + Fn(&mut C, M) -> Pin<Box<dyn Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send>>,
          M: 'static + Sync + Send + FromData,
          R: 'static + Sync + Send + Serialize,
          C: 'static + Sync + Send,{
    async fn handle<'r, 's: 'r>(&'s self, req: &'r Request<'_>, data: Data) -> Outcome<'r>{
        let pool = req.guard::<State<'_, ClientPool<C>>>().await.unwrap();
        let mut grpc_client = pool.get_client_async().await;
        let message = M::from_data(req, data).await.unwrap();
        match (self.f)(&mut grpc_client, message).await{
            Ok(res) => Outcome::from(req, Json(res.into_inner())),
            Err(e) => Outcome::Failure(rocket::http::Status::new(
                404,
                "I don't know tbh"
            )),
        }
    }
}