use actix_files as fs;
use actix_web::{error, web, App, HttpResponse, HttpServer, Responder, Result};
use clap::Parser;
use serde::Deserialize;
use tera::{Context, Tera};

/// Simple server configuration
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on, default 8080
    #[clap(short, long, default_value_t = 8080)]
    port: u16,
}

/// Index page repsonding with just a template rendering
/// The template has a form for submission that should be handled by the method `create_account`
async fn index(tera: web::Data<Tera>) -> Result<impl Responder> {
    println!("index");
    let _context = Context::new();

    let rendered = tera.render("index.html.tera", &_context).map_err(|err| {
        error::ErrorInternalServerError(format!("Failed to render template: {:?}", err))
    })?;

    Ok(HttpResponse::Ok().content_type("text/html").body(rendered))
}

#[derive(Deserialize)]
pub struct FormData {
    account_id: String,
    public_key: String,
}
// TODO: create_account method to handle the form submission
async fn create_account(
    tera: web::Data<Tera>,
    form: web::Form<FormData>,
) -> Result<impl Responder> {
    let data = form.into_inner();

    // TODO: call the function that creates the account

    let mut context = Context::new();
    context.insert("account_id", &data.account_id);
    context.insert("public_key", &data.public_key);

    match tera.render("form_success.html.tera", &context) {
        Ok(rendered) => Ok(HttpResponse::Ok().content_type("text/html").body(rendered)),
        Err(err) => Err(error::ErrorInternalServerError(format!(
            "Failed to render template: {:?}",
            err
        ))),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let tera = Tera::new("templates/**/*").unwrap();

    println!("Starting server at: http://0.0.0.0:{}", args.port);
    // TODO: CORS to deny requests from other domains
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(tera.clone()))
            .service(fs::Files::new("/assets", "assets").show_files_listing()) // for serving the static files
            .route("/", web::get().to(index))
            .route("/create_account", web::post().to(create_account))
    })
    .bind(("0.0.0.0", args.port))?
    .run()
    .await
}
