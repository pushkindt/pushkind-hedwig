use diesel::prelude::*;
use serde::Deserialize;

#[derive(Insertable)]
#[diesel(table_name = pushkind_emailer::schema::unsubscribes)]
pub struct Unsubscribe<'a> {
    pub email: &'a str,
    pub hub_id: i32,
    pub reason: Option<&'a str>,
}

#[derive(Clone, Debug, Deserialize)]
/// Basic configuration shared across handlers.
pub struct ServerConfig {
    pub domain: String,
    pub database_url: String,
    pub zmq_emailer_pub: String,
    pub zmq_emailer_sub: String,
    pub zmq_replier_pub: String,
    pub zmq_replier_sub: String,
}
