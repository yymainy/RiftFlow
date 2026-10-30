use std::any::Any;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use riftflow_shard::Instances;

struct Probe {
    hits: AtomicU64,
}

#[test]
fn instances_roundtrips_a_concrete_arc() {
    let instances = Instances::default();
    let concrete = Arc::new(Probe { hits: AtomicU64::new(7) });

    let erased = Arc::clone(&concrete) as Arc<dyn Any + Send + Sync>;
    instances.insert_dyn("probe", erased);

    assert!(instances.contains("probe"));

    let recovered = instances.get::<Probe>("probe");
    assert!(recovered.is_some(), "downcast to Arc<Probe> failed");
    assert_eq!(recovered.unwrap().hits.load(std::sync::atomic::Ordering::SeqCst), 7);

    // and the typed helper works too
    let instances2 = Instances::default();
    instances2.insert("probe2", Arc::new(Probe { hits: AtomicU64::new(1) }));
    assert!(instances2.get::<Probe>("probe2").is_some());
}
