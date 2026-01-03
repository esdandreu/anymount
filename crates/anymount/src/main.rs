use std::result::Result;

#[tokio::main]
async fn main() -> Result<(), String> {
    anymount::cli::run().await
}
