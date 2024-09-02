use postgres::{Client, NoTls, Error};
use std::env;

pub struct DbClient {
    client: Client
}

impl DbClient {
    pub fn setup(&mut self) {
        let connection = env::var("CONNECTION_STRING").is_ok();
        self.client = Client::connect(connection)
    }
}
