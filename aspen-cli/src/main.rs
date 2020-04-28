use aspen;

#[tokio::main]
async fn main() {
    let sources = (0..10000).map(|_| aspen::Source::new("test:x", "object Hello ".repeat(200))).collect();

    aspen::syntax::parse_modules(sources).await;
}
