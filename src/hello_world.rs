pub const ROUTE_PATH: &str = "/hello-world";

pub async fn get() -> &'static str {
    "Hello, World!"
}
