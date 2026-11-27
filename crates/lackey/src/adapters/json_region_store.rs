//! The regions as core's locked JSON store, `.sorcerer/state/lackey/regions.json`, and the
//! labels as JSON lines, `labels.jsonl`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use sorcerer_core::adapters::json_store::{Snapshot, Store};
use sorcerer_core::{Error, Result};

use crate::domain::region_record::{Label, RegionRecord, RegionRecords};
use crate::ports::region_store::RegionStore;

pub struct JsonRegionStore {
    store: Store<RegionRecords>,
    labels: PathBuf,
}

impl JsonRegionStore {
    pub fn at(regions: PathBuf, labels: PathBuf) -> JsonRegionStore {
        JsonRegionStore {
            store: Store::at(regions),
            labels,
        }
    }

    fn all(&self) -> RegionRecords {
        match self.store.read() {
            Ok(Snapshot::Current(records)) => records,
            _ => RegionRecords::default(),
        }
    }
}

impl RegionStore for JsonRegionStore {
    fn get(&self, id: &str) -> Option<RegionRecord> {
        self.all().regions.remove(id)
    }

    fn of_file(&self, file: &str) -> Vec<RegionRecord> {
        let mut records: Vec<RegionRecord> = self
            .all()
            .regions
            .into_values()
            .filter(|record| record.region.file == file)
            .collect();
        records.sort_by_key(|record| record.region.lines.start);
        records
    }

    fn put(&self, records: &[RegionRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        self.store.update(|kept| {
            for record in records {
                kept.regions
                    .insert(record.region.id.clone(), record.clone());
            }
        })
    }

    fn label(&self, label: &Label) -> Result<()> {
        let mut line = serde_json::to_vec(label).map_err(|e| Error::Config {
            file: self.labels.clone(),
            why: e.to_string(),
        })?;
        line.push(b'\n');
        if let Some(folder) = self.labels.parent() {
            std::fs::create_dir_all(folder).map_err(|e| Error::io(folder, e))?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.labels)
            .and_then(|mut file| file.write_all(&line))
            .map_err(|e| Error::io(&self.labels, e))
    }
}
