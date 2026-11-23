//! Games started on the CPU for the GPU batch. A game is started and turned into its device block on a worker thread, and
//! only the block is kept: it goes into a page of blocks laid end to end, which is uploaded
//! as it is (no staging copy). `GpuBatch::reset` builds its pages on scoped threads;
//! `Bank` builds them in the background from games queued ahead, so a training loop's
//! restarts of finished slots wait for the device's upload alone.
//!
//! Order is the caller's, never the threads': a page holds its games in the order they were
//! given, and the bank hands pages out in the order they were queued.

use crate::state as device;
use engine_fast::reset::{self, Config};
use engine_fast::tables::Tables;
use engine_fast_batch::Status;
use std::collections::{BTreeMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// Games per page of the bank: small enough that the first page of a new queue is ready soon,
/// large enough that an upload covers a few pages.
pub const PAGE: usize = 64;

/// What starting one game gave.
pub struct Started {
    pub config: Config,
    /// the board's tile count, or why the slot is stopped: refused (an error) or failed (a
    /// panic, with its message, as `engine_fast_batch`'s guarded keeps it)
    pub outcome: Result<usize, (Status, String)>,
}

/// Games' device blocks end to end (`words`, one block of `stride` words per game, all zero
/// for a game that did not start) and what each start gave, in the order given. `taken`
/// games from the front are already used.
pub struct Page {
    pub words: Vec<u64>,
    pub stride: usize,
    pub started: Vec<Started>,
    pub taken: usize,
}

impl Page {
    pub fn left(&self) -> usize {
        self.started.len() - self.taken
    }

    /// The blocks of games `from..to`.
    pub fn blocks(&self, from: usize, to: usize) -> &[u64] {
        &self.words[from * self.stride..to * self.stride]
    }
}

/// What every start needs: the rules and the batch's block capacities.
pub struct Rules {
    pub catalog: String,
    pub tables: Tables,
    /// tiles, units
    pub caps: (i32, i32),
}

impl Rules {
    /// Start `configs` in order into one page.
    pub fn page(&self, configs: Vec<Config>) -> Page {
        let stride = device::block_size(self.caps.0, self.caps.1) / 8;
        let mut words = vec![0u64; configs.len() * stride];
        let started = configs
            .into_iter()
            .zip(words.chunks_exact_mut(stride))
            .map(|(config, to)| {
                let outcome = self.start(&config).map(|block| {
                    to.copy_from_slice(block.words());
                    block.tile_count as usize
                });
                Started { config, outcome }
            })
            .collect();
        Page {
            words,
            stride,
            started,
            taken: 0,
        }
    }

    /// One game's block; an error refuses the slot, a panic fails it.
    fn start(&self, config: &Config) -> Result<device::Block, (Status, String)> {
        let r = catch_unwind(AssertUnwindSafe(|| {
            reset::reset(config, &self.catalog, &self.tables)
        }));
        let game = match r {
            Ok(Ok(g)) => g,
            Ok(Err(reason)) => return Err((Status::Refused, reason)),
            Err(panic) => return Err((Status::Failed, panic_text(panic.as_ref()))),
        };
        device::to_device_with(&game, self.caps.0, self.caps.1).map_err(|e| (Status::Refused, e))
    }

    /// `configs` started on `workers` scoped threads, in pages of at most `per_page` games, in
    /// order.
    pub fn pages(&self, configs: &[Config], workers: usize, per_page: usize) -> Vec<Page> {
        let chunks: Vec<&[Config]> = configs.chunks(per_page.max(1)).collect();
        let next = Mutex::new(0usize);
        let mut done: Vec<(usize, Page)> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers.clamp(1, chunks.len().max(1)))
                .map(|_| {
                    s.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            let k = {
                                let mut n = next.lock().unwrap();
                                *n += 1;
                                *n - 1
                            };
                            let Some(part) = chunks.get(k) else {
                                return mine;
                            };
                            mine.push((k, self.page(part.to_vec())));
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("a start worker"))
                .collect()
        });
        done.sort_by_key(|(k, _)| *k);
        done.into_iter().map(|(_, p)| p).collect()
    }
}

/// A panic's message, as `engine_fast_batch`'s guarded keeps it.
fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "panic".to_owned())
}

#[derive(Default)]
struct Queue {
    /// pages to build: their number and games
    jobs: VecDeque<(u64, Vec<Config>)>,
    /// pages built, by number, or the panic that stopped one (outside the guarded start)
    built: BTreeMap<u64, Result<Page, String>>,
    stop: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    /// a job was queued, or the bank is stopping
    work: Condvar,
    /// a page was built
    built: Condvar,
}

/// Games queued ahead, started on background threads.
pub struct Bank {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    /// pages queued so far, and the first page not yet handed out
    queued_pages: u64,
    next_page: u64,
    /// the page being handed out
    front: Option<Page>,
    /// games queued and not yet handed out
    left: usize,
    /// why a page could not be built: every later call fails with it, so no game is skipped
    failed: Option<String>,
}

impl Bank {
    pub fn new(rules: Arc<Rules>, workers: usize) -> Bank {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            work: Condvar::new(),
            built: Condvar::new(),
        });
        let workers = (0..workers.max(1))
            .map(|_| {
                let (shared, rules) = (shared.clone(), rules.clone());
                std::thread::spawn(move || loop {
                    let (number, configs) = {
                        let mut q = shared.queue.lock().unwrap();
                        loop {
                            if q.stop {
                                return;
                            }
                            if let Some(job) = q.jobs.pop_front() {
                                break job;
                            }
                            q = shared.work.wait(q).unwrap();
                        }
                    };
                    let page = catch_unwind(AssertUnwindSafe(|| rules.page(configs)))
                        .map_err(|p| panic_text(p.as_ref()));
                    shared.queue.lock().unwrap().built.insert(number, page);
                    shared.built.notify_all();
                })
            })
            .collect();
        Bank {
            shared,
            workers,
            queued_pages: 0,
            next_page: 0,
            front: None,
            left: 0,
            failed: None,
        }
    }

    /// Start `configs` in the background, after every game queued before them. Every queued
    /// game's block is held until it is handed out, so the caller bounds the queue.
    pub fn queue(&mut self, configs: Vec<Config>) {
        if configs.is_empty() {
            return;
        }
        self.left += configs.len();
        let mut q = self.shared.queue.lock().unwrap();
        for part in configs.chunks(PAGE) {
            q.jobs.push_back((self.queued_pages, part.to_vec()));
            self.queued_pages += 1;
        }
        drop(q);
        self.shared.work.notify_all();
    }

    /// Games queued and not yet handed out.
    pub fn len(&self) -> usize {
        self.left
    }

    pub fn is_empty(&self) -> bool {
        self.left == 0
    }

    /// Games that can be handed out without waiting: the front page's and those of the pages
    /// built after it with none missing between; an error when the next of those failed.
    pub fn ready(&self) -> Result<usize, String> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let mut n = self.front.as_ref().map_or(0, Page::left);
        let q = self.shared.queue.lock().unwrap();
        for k in self.next_page..self.queued_pages {
            match q.built.get(&k) {
                Some(Ok(p)) => n += p.left(),
                Some(Err(e)) if n == 0 => return Err(format!("starting queued games: {e}")),
                _ => break,
            }
        }
        Ok(n)
    }

    /// The page the next game comes from, waiting for it to be built; None when nothing is
    /// queued, an error when building it panicked.
    pub fn front(&mut self) -> Result<Option<&mut Page>, String> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.front.as_ref().is_some_and(|p| p.left() == 0) {
            self.front = None;
        }
        if self.front.is_none() {
            if self.next_page == self.queued_pages {
                return Ok(None);
            }
            let mut q = self.shared.queue.lock().unwrap();
            let page = loop {
                if let Some(p) = q.built.remove(&self.next_page) {
                    break p;
                }
                q = self.shared.built.wait(q).unwrap();
            };
            self.next_page += 1;
            match page {
                Ok(p) => self.front = Some(p),
                Err(e) => {
                    let e = format!("starting queued games: {e}");
                    self.failed = Some(e.clone());
                    return Err(e);
                }
            }
        }
        Ok(self.front.as_mut())
    }

    /// Mark `n` games of the front page as handed out.
    pub fn took(&mut self, n: usize) {
        let page = self.front.as_mut().expect("a front page");
        assert!(n <= page.left());
        page.taken += n;
        self.left -= n;
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().stop = true;
        self.shared.work.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = include_str!("../../rules/rules-124.json");

    fn rules() -> Arc<Rules> {
        let tables = Tables::compile(CATALOG).unwrap();
        Arc::new(Rules {
            catalog: CATALOG.to_owned(),
            tables,
            caps: device::default_caps(11 * 11),
        })
    }

    fn configs(n: i32) -> Vec<Config> {
        (0..n)
            .map(|k| Config {
                rules: 124,
                seed: 50 + k,
                size: 11,
                map: 2,
                mode: 4,
                // every seventh game names no such tribe and is refused
                tribes: if k % 7 == 3 {
                    vec![999, 999]
                } else {
                    vec![7, 2]
                },
                action_limit: 100,
            })
            .collect()
    }

    /// Each game's block and outcome, in order, from pages.
    fn flat(pages: impl IntoIterator<Item = Page>) -> Vec<(Vec<u64>, Result<usize, String>)> {
        let mut out = Vec::new();
        for p in pages {
            for (k, s) in p.started.iter().enumerate().skip(p.taken) {
                let outcome = s.outcome.clone().map_err(|(_, why)| why);
                out.push((p.blocks(k, k + 1).to_vec(), outcome));
            }
        }
        out
    }

    #[test]
    fn pages_keep_the_given_order_on_any_number_of_threads() {
        let (r, c) = (rules(), configs(20));
        let one = flat([r.page(c.clone())]);
        assert_eq!(one.len(), 20);
        assert!(one.iter().filter(|(_, o)| o.is_err()).count() == 3);
        let (tiles, units) = r.caps;
        let block = device::to_device_with(
            &reset::reset(&c[0], CATALOG, &r.tables).unwrap(),
            tiles,
            units,
        )
        .unwrap();
        assert_eq!(one[0].0, block.words(), "a page holds the game's block");
        for (workers, per_page) in [(1, 20), (4, 3), (8, 1)] {
            assert!(
                flat(r.pages(&c, workers, per_page)) == one,
                "{workers} x {per_page}"
            );
        }
    }

    #[test]
    fn the_bank_hands_out_games_in_queue_order() {
        let (r, c) = (rules(), configs(150));
        let mut bank = Bank::new(r.clone(), 4);
        assert!(bank.front().unwrap().is_none());
        bank.queue(c[..70].to_vec());
        bank.queue(c[70..].to_vec());
        assert_eq!(bank.len(), 150);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while bank.ready().unwrap() < 150 {
            assert!(
                std::time::Instant::now() < deadline,
                "the bank never filled"
            );
            std::thread::yield_now();
        }
        let mut got = Vec::new();
        // taken in uneven parts, as restarts take them
        for want in [5, 64, 1, 30, 100] {
            let mut left = want;
            while left > 0 {
                let Some(page) = bank.front().unwrap() else {
                    break;
                };
                let take = page.left().min(left);
                let first = page.taken;
                for k in first..first + take {
                    let outcome = page.started[k].outcome.clone().map_err(|(_, why)| why);
                    got.push((page.blocks(k, k + 1).to_vec(), outcome));
                }
                bank.took(take);
                left -= take;
            }
        }
        assert!(bank.is_empty() && bank.front().unwrap().is_none());
        assert!(got == flat([r.page(c)]));
    }
}
