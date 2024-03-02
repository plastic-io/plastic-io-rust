# Plastic IO

PlasticIO is a Rust-based hierarchial hyper graph framework designed to facilitate the creation and management of dynamic graph structures, particularly focusing on the execution and interaction of nodes within these graphs. It leverages Rust's powerful concurrency features to ensure thread-safe operation and efficient event handling.

# Features
* Event-Driven Architecture: Utilizes an EventEmitter pattern to enable decoupled communication between different parts of the system through events.
* Thread-Safe Operation: Ensures safe concurrent access and modification of shared resources using Rust's synchronization primitives.
* Dynamic Graph Management: Supports the creation, modification, and execution of interconnected graph structures at runtime.
* Flexible Graph Nodes: Nodes within graphs can be linked, allowing for complex data flow and transformation pipelines.
* Integrated JavaScript Execution: Employs the V8 engine to execute JavaScript code within nodes, enabling dynamic script execution in response to events.


## Getting Started
### Prerequisites
Rust toolchain (latest stable version recommended)
Cargo (Rust's package manager)
Installation
Clone the repository:

```
  git clone https://github.com/your-username/plastic_io.git
  cd plastic_io
```
Build the project:

```
  cargo build
```

Run the project:
```
cargo run -- [OPTIONS]
```

## Basic Usage

Managing Graphs
Load, modify, and store graphs using the provided functions. Graphs can be manipulated both programmatically and through JSON interfaces.

```
use plastic_io::loader::{set_graph_to_global_store, get_graph_from_global_store};

let graph = Graph::new(); // Assume this is a valid graph instance
set_graph_to_global_store(graph.clone());

if let Some(retrieved_graph) = get_graph_from_global_store(&graph.id) {
    // Work with the retrieved graph
}
```

## Library - Running Examples

```

  use plastic_io::loader::file;
  use plastic_io::utils::set_verbosity;
  use plastic_io::Scheduler;
  use plastic_io::types::{Event, EventType};

  let graph = file("tests/fixtures/graphs/graph_linked.json");
  let scheduler = Scheduler::new(graph, None);

  scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
      let _ = tx.try_send(event);
  });

  let test_value = "foo";
  let test_return_value = "foo bar";

  scheduler.url(
      "index".to_string(),
      serde_json::Value::String(test_value.to_string()),
      "field".to_string(),
  );

  let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
      .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
  let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
  assert_eq!(value, test_return_value, "Expected to see another value here.");

```




## CLI - Running Examples
To run examples, use Cargo's run command with the appropriate flags to specify the graph source and other options.

```
cargo run -- --path "path/to/graph.json"
```


### Acknowledgments
The Rust Community for the invaluable resources and support.
The developers of the V8 engine and the serde, tokio, and clap crates for their fantastic work.
