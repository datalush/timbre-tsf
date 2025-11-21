use arrow::ipc::reader::FileReader;
use std::fs::File;
use std::time::Instant;
use timbre_tsf::arrow::FromArrowConverter;

const DATASET_PATH: &str = "data/iot_dataset.arrow";

fn main() {
    println!("Loading dataset...");
    let file = File::open(DATASET_PATH).expect("Failed to open dataset");
    let reader = FileReader::try_new(file, None).expect("Failed to create reader");
    let batches: Vec<_> = reader
        .collect::<Result<_, _>>()
        .expect("Failed to read batches");

    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    println!("Loaded {} rows\n", total_rows);

    println!("Running Timbre write...");
    let start = Instant::now();

    let mut converter = FromArrowConverter::builder("/tmp/profile_perf.timbre")
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .expect("Failed to create converter");

    for batch in &batches {
        converter.write_batch(batch).expect("Failed to write batch");
    }

    converter.finish().expect("Failed to finish");

    let elapsed = start.elapsed();
    let throughput = 1040.0 / elapsed.as_secs_f64();

    println!("\nWrite time: {:?}", elapsed);
    println!("Throughput: {:.2} MB/s", throughput);

    // Keep process alive for perf to capture
    std::thread::sleep(std::time::Duration::from_secs(5));
}
