use diesel::prelude::*;

#[derive(Insertable)]
#[diesel(table_name = pushkind_common::schema::emailer::unsubscribes)]
pub struct Unsubscribe<'a> {
    pub email: &'a str,
    pub hub_id: i32,
    pub reason: Option<&'a str>,
}
