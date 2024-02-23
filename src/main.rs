mod scheduler;
use clap::Parser;

// Graph parser
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Graph JSON
    #[arg(short, long)]
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
    match scheduler::parse_graph(&args.graph) {
        Ok(graph) => {
            let scheduler = scheduler::Scheduler::new(graph, None);
            scheduler
            .event_emitter
            .subscribe(scheduler::EventType::AfterSet, |event| {
                let data = &event.data;
                    if let Some(return_value) = data.get("return") {
                        print!("{}", return_value);
                    }
            });
            scheduler.url(args.url, serde_json::Value::String(args.value.to_string()), args.field);
        },
        Err(e) => {
            eprintln!("Error parsing JSON into Graph: {}", e);
            std::process::exit(1);
        }
    }
}