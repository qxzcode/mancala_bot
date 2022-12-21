use std::{
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use egui::{mutex::Mutex, Context};

use crate::{
    game_state::GameState,
    mcts::{MCTSContext, StateStats},
};

/// A message from the main thread to the worker thread.
enum Message {
    /// Stop the worker thread.
    Stop,

    /// Clear the explored node cache.
    ClearCache,

    /// Set the active game state to work on.
    SetActiveState(GameState),
}

/// Data representing the state of the worker thread's computation and results
/// on the active game state.
#[derive(Clone)]
pub struct WorkerStateData {
    pub game_state: GameState,
    pub stats: StateStats,
}

/// Shared data on the overall state of the worker thread.
#[derive(Clone)]
pub struct WorkerData {
    pub cache_size: usize,
    pub cache_size_limit: usize,
    pub samples_per_second: f32,
    pub average_search_depth: f32,
}

/// Manages the worker thread performing game computations and facilitates
/// inter-thread communicaiton.
pub struct Worker {
    /// The join handle for the worker thread.
    join_handle: Option<JoinHandle<()>>,

    /// Sender for sending control messages to the worker thread.
    message_sender: Sender<Message>,

    /// The latest state data from the worker thread.
    cur_state_data: Arc<Mutex<Option<WorkerStateData>>>,

    /// The shared overall data for the worker thread.
    cur_data: Arc<Mutex<WorkerData>>,
}

impl Worker {
    /// Spawns a new worker thread and returns a `Worker` manager for it.
    #[must_use]
    pub fn spawn(ui_context: &Context, cache_size_limit: usize) -> Self {
        let cur_state_data = Arc::new(Mutex::new(None));
        let cur_state_data2 = cur_state_data.clone();

        let cur_data = Arc::new(Mutex::new(WorkerData {
            cache_size: 0,
            cache_size_limit,
            samples_per_second: 0.0,
            average_search_depth: 0.0,
        }));
        let cur_data2 = cur_data.clone();

        let (sender, receiver) = mpsc::channel();

        let ui_context = ui_context.clone();

        let join_handle = thread::Builder::new()
            .name("worker".into())
            .spawn(move || {
                println!("Worker thread started");
                let update_delay = Duration::from_secs_f64(1.0 / 60.0); // delay between UI updates
                let mut mcts_context = MCTSContext::new(cache_size_limit);
                let mut active_game_state = None;

                let send_update = |mcts_context: &MCTSContext, game_state: &GameState| {
                    let new_state_data =
                        mcts_context
                            .stats_for(game_state)
                            .map(|stats| WorkerStateData {
                                game_state: game_state.clone(),
                                stats: stats.clone(),
                            });
                    *cur_state_data2.lock() = new_state_data;
                    cur_data2.lock().cache_size = mcts_context.cache_size();
                    ui_context.request_repaint();
                };

                let mut last_sps_reading = Instant::now();
                let mut num_samples = 0;
                let mut sum_depths = 0;

                'main_loop: loop {
                    // handle any messages sent from the main thread
                    for message in receiver.try_iter() {
                        match message {
                            Message::Stop => break 'main_loop,
                            Message::ClearCache => mcts_context.clear_cache(),
                            Message::SetActiveState(game_state) => {
                                send_update(&mcts_context, &game_state);
                                active_game_state = Some(game_state);
                            }
                        }
                    }

                    match &active_game_state {
                        Some(game_state) if game_state.result().is_none() => {
                            // do some MCTS computation
                            mcts_context.cache_size_limit = cur_data2.lock().cache_size_limit;
                            let (ponder_num_samples, ponder_sum_depths) =
                                mcts_context.ponder(game_state, update_delay);
                            num_samples += ponder_num_samples;
                            sum_depths += ponder_sum_depths;

                            // update the state data that the main thread can access
                            send_update(&mcts_context, game_state);
                        }
                        _ => thread::sleep(update_delay),
                    }

                    let elapsed = last_sps_reading.elapsed();
                    if elapsed > Duration::from_secs_f32(1.0) {
                        let new_sps = num_samples as f32 / elapsed.as_secs_f32();
                        let new_asd = if num_samples == 0 {
                            0.0
                        } else {
                            sum_depths as f32 / num_samples as f32
                        };
                        num_samples = 0;
                        sum_depths = 0;
                        last_sps_reading = Instant::now();

                        let mut data = cur_data2.lock();
                        if data.samples_per_second != new_sps {
                            data.samples_per_second = new_sps;
                            data.average_search_depth = new_asd;
                            ui_context.request_repaint();
                        }
                    }
                }
            })
            .expect("failed to spawn worker thread");

        Self {
            join_handle: Some(join_handle),
            message_sender: sender,
            cur_state_data,
            cur_data,
        }
    }

    /// Sets the active game state that the worker should compute on.
    pub fn set_active_state(&self, game_state: GameState) {
        self.message_sender
            .send(Message::SetActiveState(game_state))
            .expect("failed to send to worker thread");
    }

    /// Clears the explored node cache.
    pub fn clear_cache(&self) {
        self.message_sender
            .send(Message::ClearCache)
            .expect("failed to send to worker thread");
    }

    /// Returns the current worker state data.
    #[must_use]
    pub fn state_data(&self) -> Option<WorkerStateData> {
        self.cur_state_data.lock().clone()
    }

    /// Returns the current size of the worker node cache.
    #[must_use]
    pub fn cache_size(&self) -> usize {
        self.cur_data.lock().cache_size
    }

    /// Returns the size limit for the worker node cache.
    #[must_use]
    pub fn cache_size_limit(&self) -> usize {
        self.cur_data.lock().cache_size_limit
    }

    /// Sets the size limit for the worker node cache.
    pub fn set_cache_size_limit(&self, cache_size_limit: usize) {
        self.cur_data.lock().cache_size_limit = cache_size_limit;
    }

    /// Returns the worker's current sample rate.
    #[must_use]
    pub fn samples_per_second(&self) -> f32 {
        self.cur_data.lock().samples_per_second
    }

    /// Returns the worker's current average search depth.
    #[must_use]
    pub fn average_search_depth(&self) -> f32 {
        self.cur_data.lock().average_search_depth
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.message_sender.send(Message::Stop);
        self.join_handle
            .take()
            .unwrap()
            .join()
            .expect("worker thread should not panic");
    }
}
