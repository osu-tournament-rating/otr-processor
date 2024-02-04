mod constants;
mod api;
mod env;

use tokio;


#[tokio::main]
async fn main() {
    let login_res = api::login().await;

    println!("Login result: {:?}", login_res)
}
