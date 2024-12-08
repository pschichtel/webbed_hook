use actix_web::{post, App, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web::web;
use serde_json::Value;

#[post("/validate")]
async fn hello<'a>(req: HttpRequest, body: web::Json<Value>) -> impl Responder {
    println!("REQ: {:?} with body: {:?}", req, body.0);
    HttpResponse::NoContent()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .service(hello)
    })
        .bind(("0.0.0.0", 8080))?
        .run()
        .await
}