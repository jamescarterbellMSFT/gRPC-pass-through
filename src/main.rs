#![feature(proc_macro_hygiene, decl_macro, async_closure)]

#[macro_use] extern crate rocket;

use std::future::Future;
use rocket::{Route, Request, Data, State, handler::{Outcome, Handler}, data::{FromTransformedData}, http::Method};
use rocket_contrib::json::Json;
use corp::vault_client::VaultClient;
use tonic::transport::channel::Channel;
use crossbeam::channel::{bounded, Sender, Receiver};
use serde::{de::DeserializeOwned, Serialize};
use std::pin::Pin;


pub mod corp{
    tonic::include_proto!("corp");
}

 
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>  {
    let t = &Box::pin(|c: &'_ mut VaultClient<Channel>, m: corp::DepositRequest| async {VaultClient::<Channel>::deposit(c, m).await});
    let route = GrpcRoute::build_route(
        t,
        Method::Put,
        "/deposit"
    );
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


struct GrpcHandler< C, M, R, F, Q>
where F: 'static + Send + Sync + Copy + Fn(& mut C, M) -> Q, 
      Q: 'static + Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send + Sized,
      M: 'static + Send + DeserializeOwned,
      R: 'static + Send + Serialize,
      C: 'static + Send + Sync,{
    f: Pin<Box<F>>,
    c: std::marker::PhantomData<C>,
    m: std::marker::PhantomData<M>,
    r: std::marker::PhantomData<R>,
    q: std::marker::PhantomData<Q>,
}

unsafe impl< C, M, R, F, Q> Sync for GrpcHandler< C, M, R, F, Q>
where F: 'static + Send + Sync + Copy + Fn(& mut C, M) -> Q, 
      Q: 'static + Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send + Sized,
      M: 'static + Send + DeserializeOwned,
      R: 'static + Send + Serialize,
      C: 'static + Send + Sync,{}


impl< C, M, R, F, Q> Clone for GrpcHandler< C, M, R, F, Q>
where F: 'static + Send + Sync + Copy + Fn(& mut C, M) -> Q, 
      Q: 'static + Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send + Sized,
      M: 'static + Send + DeserializeOwned,
      R: 'static + Send + Serialize,
      C: 'static + Send + Sync,{

    fn clone(&self) -> Self{
        Self{
            f: Box::pin(*self.f),
            c: std::marker::PhantomData,
            m: std::marker::PhantomData,
            q: std::marker::PhantomData,
            r: std::marker::PhantomData
        }
    }
}


#[rocket::async_trait]
impl< C, M, R, F, Q> Handler for GrpcHandler< C, M, R, F, Q>
    where F: 'static + Send + Sync + Copy + Fn(& mut C, M) -> Q, 
          Q: 'static + Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send + Sized,
          M: 'static + Send + DeserializeOwned,
          R: 'static + Send + Serialize,
          C: 'static + Send + Sync,{
    async fn handle<'r, 's: 'r>(&'s self, req: &'r Request<'_>, data: Data) -> Outcome<'r>{
        let pool = req.guard::<State<'_, ClientPool<C>>>().await.unwrap();
        let mut grpc_client = pool.get_client_async().await;
        let message = serde_json::from_str(&Json::<M>::transform(req, data).await.owned().unwrap().clone()).unwrap();
        match (self.f)(&mut grpc_client, message).await{
            Ok(res) => Outcome::from(req, Json(res.into_inner())),
            Err(e) => Outcome::Failure(rocket::http::Status::new(
                404,
                "I don't know tbh"
            )),
        }
    }
}

trait GrpcRoute< C, M, R, Q, F>
where F: 'static + Send + Sync + Copy + Fn(& mut C, M) -> Q, 
      Q: 'static + Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send + Sized,
      M: 'static + Send + DeserializeOwned,
      R: 'static + Send + Serialize,
      C: 'static + Send + Sync,{

    fn build_handler(&self) -> GrpcHandler< C, M, R, F, Q>;

    fn build_route<S>(&self, method: Method, path: S) -> Route
        where S: AsRef<str>;
}

impl< F, C, M, R, Q> GrpcRoute< C, M, R, Q, F> for Pin<Box<F>>
    where F: 'static + Send + Sync + Copy + Fn(& mut C, M) -> Q, 
          Q: 'static + Future<Output = Result<tonic::Response<R>, tonic::Status>> + Sync + Send + Sized,
          M: 'static + Send + DeserializeOwned,
          R: 'static + Send + Serialize,
          C: 'static + Send + Sync,{

    fn build_handler(&self) -> GrpcHandler< C, M, R, F, Q>{
        GrpcHandler{
            f: self.clone(),
            c: std::marker::PhantomData,
            m: std::marker::PhantomData,
            q: std::marker::PhantomData,
            r: std::marker::PhantomData,
        }
    }

    fn build_route<S>(&self, method: Method, path: S) -> Route
        where S: AsRef<str>{
        let handler = self.build_handler();
        Route::new(method, path, handler)
    }
}
