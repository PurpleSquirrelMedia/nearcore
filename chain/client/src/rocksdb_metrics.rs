use near_metrics::{try_create_gauge_vec, try_create_int_gauge};
use near_store::db::{StatsValue, StoreStatistics};
use once_cell::sync::Lazy;
use prometheus::{GaugeVec, IntGauge};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::warn;

pub(crate) fn export_stats_as_metrics(stats: StoreStatistics) {
    match ROCKSDB_METRICS.lock().unwrap().export_stats_as_metrics(stats) {
        Ok(_) => {}
        Err(err) => {
            warn!(target:"stats", "Failed to export store statistics: {:?}",err);
        }
    }
}

/// Wrapper for re-exporting RocksDB stats into Prometheus metrics.
static ROCKSDB_METRICS: Lazy<Mutex<RocksDBMetrics>> =
    Lazy::new(|| Mutex::new(RocksDBMetrics::default()));

#[derive(Default, Debug)]
/// Creates prometheus metrics on-demand for exporting RocksDB statistics.
pub(crate) struct RocksDBMetrics {
    // Contains counters and sums, which are integer statistics in RocksDB.
    int_gauges: HashMap<String, IntGauge>,
    // Contains floating point statistics, such as quantiles of timings.
    gauges: HashMap<String, GaugeVec>,
}

impl RocksDBMetrics {
    pub fn export_stats_as_metrics(
        &mut self,
        stats: StoreStatistics,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (stat_name, values) in stats.data {
            if values.len() == 1 {
                // A counter stats.
                // A statistic 'a.b.c' creates the following prometheus metric:
                // - near_a_b_c
                if let StatsValue::Count(value) = values[0] {
                    self.set_int_value(
                        |stat_name: &str| stat_name.to_string(),
                        |stat_name| get_prometheus_metric_name(stat_name),
                        &stat_name,
                        value,
                    )?;
                }
            } else {
                // A summary stats.
                // A statistic 'a.b.c' creates the following prometheus metrics:
                // - near_a_b_c_sum
                // - near_a_b_c_count
                // - near_a_b_c{quantile="0.95"}
                for stats_value in values {
                    match stats_value {
                        StatsValue::Count(value) => {
                            self.set_int_value(
                                get_stats_summary_count_key,
                                get_metric_name_summary_count_gauge,
                                &stat_name,
                                value,
                            )?;
                        }
                        StatsValue::Sum(value) => {
                            self.set_int_value(
                                get_stats_summary_sum_key,
                                get_metric_name_summary_sum_gauge,
                                &stat_name,
                                value,
                            )?;
                        }
                        StatsValue::Percentile(percentile, value) => {
                            let key = &stat_name;

                            let gauge = match self.gauges.entry(key.to_string()) {
                                Entry::Vacant(entry) => entry.insert(try_create_gauge_vec(
                                    &get_prometheus_metric_name(&stat_name),
                                    &stat_name,
                                    &["quantile"],
                                )?),
                                Entry::Occupied(entry) => entry.into_mut(),
                            };
                            gauge
                                .with_label_values(&[&format!("{:.2}", percentile as f64 * 0.01)])
                                .set(value);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// `stat_name` is the name of the statistics at the storage level.
    /// `metric_fn` returns a name of the prometheus metric that re-exports that statistic.
    /// `key_fn` returns a hashmap key for the hashmaps in the ROCKSDB_METRICS singleton.
    fn set_int_value(
        &mut self,
        key_fn: fn(&str) -> String,
        metric_fn: fn(&str) -> String,
        stat_name: &str,
        value: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let key = key_fn(stat_name);
        let gauge = match self.int_gauges.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(try_create_int_gauge(&metric_fn(stat_name), stat_name)?)
            }
            Entry::Occupied(entry) => entry.into_mut(),
        };
        gauge.set(value);
        Ok(())
    }
}

fn get_prometheus_metric_name(stat_name: &str) -> String {
    format!("near_{}", stat_name.replace('.', "_"))
}

fn get_metric_name_summary_count_gauge(stat_name: &str) -> String {
    format!("near_{}_count", stat_name.replace('.', "_"))
}

fn get_metric_name_summary_sum_gauge(stat_name: &str) -> String {
    format!("near_{}_sum", stat_name.replace('.', "_"))
}

fn get_stats_summary_count_key(stat_name: &str) -> String {
    format!("{}.count", stat_name)
}

fn get_stats_summary_sum_key(stat_name: &str) -> String {
    format!("{}.sum", stat_name)
}
