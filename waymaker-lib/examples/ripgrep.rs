use waymaker::Result;
use waymaker::nucleo::Worker;

fn main() -> Result<()> {
    let worker = Worker::new_single_column();
    let items = vec!["item1", "item2", "item3"];
    worker.append(items);

    Ok(())
}
