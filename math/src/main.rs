mod api;
mod app;
mod details;
mod terminal;
mod wizard;

#[cfg(test)]
mod tests;

use std::io;

use app::App;
use terminal::TerminalGuard;

const DEFAULT_API_URL: &str = "http://localhost:3000";

#[tokio::main]
async fn main() -> io::Result<()> {
    let _terminal = TerminalGuard::enter()?;
    let api_url = std::env::var("MATH_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
    App::new(api_url).run().await
}
