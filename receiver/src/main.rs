use actix_web::web;
use actix_web::{post, App, HttpRequest, HttpServer, Responder};
use webbed_hook_core::webhook::{WebhookRequest, WebhookResponse};

#[post("/validate")]
async fn hello(req: HttpRequest, body: web::Json<WebhookRequest>) -> impl Responder {
    let payload = body.0;
    println!("REQ: {:?} with body: {:?}", req, payload);
    web::Json(WebhookResponse::default())
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