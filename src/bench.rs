//! An opt-in, short integer-throughput measurement with no proprietary scoring.
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::time::{Duration, Instant};

const PHASE_DURATION: Duration = Duration::from_millis(600);
const MAX_THREADS: usize = 16;
// A time/cancellation check every 256 dependent mixes stays well below 20 ms
// even in debug builds. The scheduler can still delay a worker's execution.
const BATCH_MIXES: u64 = 256;
const ALGORITHM: &str = "RUST-CPU mix64 v1: 1 op = add + 2 wrapping multiplies + 3 xor-shifts; dependent u64 stream; compare only same app/build version";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CpuBenchmark {
    pub single_ops_per_sec: f64,
    pub multi_ops_per_sec: f64,
    pub threads: usize,
    pub duration_secs: f64,
    pub algorithm: String,
}

/// Runs two bounded 600 ms phases. Must only be invoked after an explicit request.
/// This measures this integer kernel, not general CPU or LLM performance.
pub fn run(cancel: Arc<AtomicBool>) -> Result<CpuBenchmark> {
    let started = Instant::now();
    let threads = worker_limit();
    let single_ops_per_sec = measure_for(PHASE_DURATION, 1, Arc::clone(&cancel))?;
    let multi_ops_per_sec = measure_for(PHASE_DURATION, threads, cancel)?;
    Ok(CpuBenchmark {
        single_ops_per_sec,
        multi_ops_per_sec,
        threads,
        duration_secs: started.elapsed().as_secs_f64(),
        algorithm: ALGORITHM.into(),
    })
}

fn worker_limit() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, MAX_THREADS)
}

// SplitMix64's integer avalanche, with state dependency retained between mixes.
// Wrapping arithmetic makes the workload identical in debug and release builds.
fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Measures completed mixes against the actual wall interval, including phase
/// synchronization and worker completion. Thread creation is outside that interval.
fn measure_for(duration: Duration, threads: usize, cancel: Arc<AtomicBool>) -> Result<f64> {
    if threads == 0 || threads > worker_limit() {
        bail!(
            "Numero di thread benchmark non valido (1..={}).",
            worker_limit()
        );
    }
    if duration.is_zero() || duration > Duration::from_secs(1) {
        bail!("Ogni fase benchmark deve durare più di zero e al massimo 1 secondo.");
    }
    if cancel.load(Ordering::Relaxed) {
        bail!("Benchmark CPU annullato.");
    }

    // A launch gate lets us abort safely if OS thread creation fails. Workers only
    // enter the barrier after every thread was created successfully.
    let gate = Arc::new((Mutex::new(None::<Option<Instant>>), Condvar::new()));
    let barrier = Arc::new(Barrier::new(threads + 1));
    let mut handles = Vec::with_capacity(threads);
    for worker in 0..threads {
        let worker_gate = Arc::clone(&gate);
        let worker_barrier = Arc::clone(&barrier);
        let worker_cancel = Arc::clone(&cancel);
        let handle = std::thread::Builder::new()
            .name(format!("rust-cpu-bench-{worker}"))
            .spawn(move || {
                let (mutex, condition) = &*worker_gate;
                let Ok(mut launch) = mutex.lock() else {
                    return 0;
                };
                while launch.is_none() {
                    let Ok(next) = condition.wait(launch) else {
                        return 0;
                    };
                    launch = next;
                }
                let Some(start) = *launch.as_ref().expect("launch gate initialized") else {
                    return 0;
                };
                drop(launch);
                worker_barrier.wait();
                let deadline = start + duration;
                let mut state = black_box(0x243f_6a88_85a3_08d3_u64.wrapping_add(worker as u64));
                let mut operations = 0_u64;
                while Instant::now() < deadline && !worker_cancel.load(Ordering::Relaxed) {
                    for _ in 0..BATCH_MIXES {
                        state = mix64(state);
                    }
                    state = black_box(state);
                    operations = operations.saturating_add(BATCH_MIXES);
                }
                black_box(state);
                operations
            });
        match handle {
            Ok(handle) => handles.push(handle),
            Err(error) => {
                let (mutex, condition) = &*gate;
                if let Ok(mut launch) = mutex.lock() {
                    *launch = Some(None);
                    condition.notify_all();
                }
                for handle in handles {
                    let _ = handle.join();
                }
                return Err(anyhow!(
                    "Creazione dei thread benchmark non riuscita: {error}"
                ));
            }
        }
    }

    let started = Instant::now();
    let (mutex, condition) = &*gate;
    {
        let mut launch = mutex
            .lock()
            .map_err(|_| anyhow!("Sincronizzazione benchmark non riuscita."))?;
        *launch = Some(Some(started));
        condition.notify_all();
    }
    barrier.wait();
    let mut operations = 0_u64;
    for handle in handles {
        operations = operations.saturating_add(
            handle
                .join()
                .map_err(|_| anyhow!("Un thread benchmark CPU si è interrotto."))?,
        );
    }
    let elapsed = started.elapsed().as_secs_f64();
    if cancel.load(Ordering::Relaxed) {
        bail!("Benchmark CPU annullato.");
    }
    if operations == 0 || elapsed <= 0.0 {
        bail!("Intervallo benchmark insufficiente: riprovare.");
    }
    Ok(operations as f64 / elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_kernel_is_deterministic_and_uses_wrapping_arithmetic() {
        assert_eq!(mix64(0), 0xe220_a839_7b1d_cdaf);
        assert_eq!(mix64(u64::MAX), 0xe4d9_7177_1b65_2c20);
        assert_ne!(mix64(1), mix64(2));
    }

    #[test]
    fn short_measurement_has_positive_finite_throughput() {
        let result = measure_for(
            Duration::from_millis(20),
            1,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(result.is_finite() && result > 0.0);
    }

    #[test]
    fn workers_and_phase_duration_are_bounded() {
        assert!((1..=MAX_THREADS).contains(&worker_limit()));
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(measure_for(Duration::from_millis(20), 0, Arc::clone(&cancel)).is_err());
        assert!(
            measure_for(
                Duration::from_millis(20),
                MAX_THREADS + 1,
                Arc::clone(&cancel)
            )
            .is_err()
        );
        assert!(measure_for(Duration::ZERO, 1, Arc::clone(&cancel)).is_err());
        assert!(measure_for(Duration::from_secs(2), 1, cancel).is_err());
    }

    #[test]
    fn cancellation_is_checked_before_starting_workers() {
        let cancel = Arc::new(AtomicBool::new(true));
        assert!(
            measure_for(Duration::from_millis(20), 1, cancel)
                .unwrap_err()
                .to_string()
                .contains("annullato")
        );
    }

    #[test]
    fn cancellation_interrupts_a_running_measurement() {
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancel);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            trigger.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = measure_for(Duration::from_millis(100), 1, cancel);
        timer.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("annullato"));
        // Allow scheduler variance in CI; the kernel checks every 256 mixes.
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
