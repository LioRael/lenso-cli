mod business;
#[path = "generated/assembly/app.rs"]
mod generated;

fn main() -> std::io::Result<()> {
    let app = generated::build()?;
    for (name, greeter) in app.greeters {
        println!("{name}: {}", greeter.greet("Lenso")?);
    }
    Ok(())
}
