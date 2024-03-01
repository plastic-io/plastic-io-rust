mod scheduler;
mod utils;
mod loader;
mod types;
use clap::Parser;
use crate::types::Graph;

// Graph parser
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Graph path
    #[arg(short, long, default_value = "")]
    path: String,
    /// Graph JSON
    #[arg(short, long, default_value = "")]
    graph: String,
    /// Node URL Entrypoint
    #[arg(short, long, default_value = "main")]
    url: String,
    /// Value to pass entrypoint node
    #[arg(short, long, default_value = "")]
    value: String,
    /// Input field name
    #[arg(short, long, default_value = "main")]
    field: String,
}

fn main() {
    let args = Args::parse();
    let graph: Graph = if &args.graph.len() > &0 {
        loader::parse_graph(&args.graph).expect("Error parsing graph JSON")
    } else if &args.path.len() > &0 {
        loader::load_graph_from_file(&args.path)
    } else {
        println!("No graph source defined.  You must define either --path or --graph.");
        return;
    };

    let scheduler = types::Scheduler::new(graph, None);
    scheduler
    .event_emitter
    .subscribe(types::EventType::AfterSet, |event| {
        let data = &event.data;
            if let Some(return_value) = data.get("return") {
                print!("{}", return_value);
            }
    });
    scheduler.url(args.url, serde_json::Value::String(args.value.to_string()), args.field);
}