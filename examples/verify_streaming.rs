use substrate::activations::cone::ConeMethod;

fn main() {
    let schemas = ConeMethod::method_schemas();
    for s in &schemas {
        println!("{}: streaming={}", s.name, s.streaming);
    }
    
    // Also print JSON
    let chat = schemas.iter().find(|s| s.name == "chat").unwrap();
    println!("\nChat JSON:");
    println!("{}", serde_json::to_string_pretty(chat).unwrap());
}
