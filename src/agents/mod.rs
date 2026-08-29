mod jira;
mod ollama;

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use tokio::sync::watch;

use crate::suggestions::SuggestionSet;

pub struct SuggestionAgent {
    running_agent: Option<RunningAgent>,
}

struct RunningAgent {
    result_receiver: Receiver<Result<AgentResult>>,
    cancellation_sender: watch::Sender<bool>,
    worker: JoinHandle<()>,
}

impl SuggestionAgent {
    pub fn new() -> Self {
        Self {
            running_agent: None,
        }
    }

    pub fn start(&mut self, request: AgentRequest) -> Result<()> {
        if self.running_agent.is_some() {
            return Err(anyhow!("a suggestion job is already running"));
        }

        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let (cancellation_sender, cancellation_receiver) = watch::channel(false);
        let worker = thread::Builder::new()
            .name("suggestion-agent".to_owned())
            .spawn(move || {
                let result = ollama::generate_suggestions(request, cancellation_receiver);
                let _ = result_sender.send(result);
            })
            .context("could not start the suggestion agent thread")?;
        self.running_agent = Some(RunningAgent {
            result_receiver,
            cancellation_sender,
            worker,
        });

        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running_agent.is_some()
    }

    pub fn try_finish(&mut self) -> Option<Result<AgentResult>> {
        let result = match self.running_agent.as_ref()?.result_receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(anyhow!(
                "suggestion agent stopped without returning a result"
            )),
        };

        let running_agent = self
            .running_agent
            .take()
            .expect("a suggestion agent result requires a running agent");

        match running_agent.worker.join() {
            Ok(()) => Some(result),
            Err(_) => Some(Err(anyhow!("suggestion agent thread panicked"))),
        }
    }
}

impl Drop for SuggestionAgent {
    fn drop(&mut self) {
        let Some(running_agent) = self.running_agent.take() else {
            return;
        };

        let _ = running_agent.cancellation_sender.send(true);
        let _ = running_agent.worker.join();
    }
}

pub struct AgentRequest {
    pub date: NaiveDate,
    pub range_start: SystemTime,
    pub range_finish: SystemTime,
    pub five_minute_intervals: Vec<AgentInterval>,
    pub fifteen_minute_intervals: Vec<AgentInterval>,
}

impl AgentRequest {
    pub fn new(
        date: NaiveDate,
        range_start: SystemTime,
        range_finish: SystemTime,
        five_minute_intervals: Vec<AgentInterval>,
        fifteen_minute_intervals: Vec<AgentInterval>,
    ) -> Self {
        Self {
            date,
            range_start,
            range_finish,
            five_minute_intervals,
            fifteen_minute_intervals,
        }
    }
}

pub struct AgentInterval {
    pub start: SystemTime,
    pub finish: SystemTime,
    pub contexts: Vec<AgentIntervalContext>,
}

impl AgentInterval {
    pub fn new(start: SystemTime, finish: SystemTime, contexts: Vec<AgentIntervalContext>) -> Self {
        Self {
            start,
            finish,
            contexts,
        }
    }
}

pub struct AgentIntervalContext {
    pub duration: Duration,
    pub executable: String,
    pub description: String,
}

impl AgentIntervalContext {
    pub fn new(duration: Duration, executable: String, description: String) -> Self {
        Self {
            duration,
            executable,
            description,
        }
    }
}

pub struct AgentResult {
    pub date: NaiveDate,
    pub range_start: SystemTime,
    pub range_finish: SystemTime,
    pub suggestions: SuggestionSet,
}

impl AgentResult {
    fn new(request: &AgentRequest, suggestions: SuggestionSet) -> Self {
        Self {
            date: request.date,
            range_start: request.range_start,
            range_finish: request.range_finish,
            suggestions,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant, UNIX_EPOCH};

    use chrono::NaiveDate;
    use tokio::sync::watch;

    use super::{AgentRequest, RunningAgent, SuggestionAgent};

    #[test]
    fn an_empty_suggestion_job_completes_and_clears_the_running_state() {
        let mut agent = SuggestionAgent::new();

        agent.start(empty_request()).unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        let result = loop {
            if let Some(result) = agent.try_finish() {
                break result.unwrap();
            }

            assert!(Instant::now() < deadline, "suggestion job did not finish");
            thread::yield_now();
        };

        assert!(result.suggestions.five_minute_suggestions.is_empty());
        assert!(result.suggestions.fifteen_minute_suggestions.is_empty());
        assert!(!agent.is_running());
    }

    #[test]
    fn starting_a_second_job_while_one_is_uncollected_is_rejected() {
        let mut agent = SuggestionAgent::new();

        agent.start(empty_request()).unwrap();

        let error = agent.start(empty_request()).unwrap_err();
        assert!(error.to_string().contains("already running"));
    }

    #[test]
    fn dropping_a_running_agent_cancels_and_joins_its_worker() {
        let (_result_sender, result_receiver) = mpsc::channel();
        let (cancellation_sender, mut cancellation_receiver) = watch::channel(false);
        let (finished_sender, finished_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(async {
                    cancellation_receiver
                        .wait_for(|cancelled| *cancelled)
                        .await
                        .unwrap();
                });
            finished_sender.send(()).unwrap();
        });
        let agent = SuggestionAgent {
            running_agent: Some(RunningAgent {
                result_receiver,
                cancellation_sender,
                worker,
            }),
        };

        drop(agent);

        finished_receiver.try_recv().unwrap();
    }

    fn empty_request() -> AgentRequest {
        AgentRequest::new(
            NaiveDate::from_ymd_opt(2026, 8, 29).unwrap(),
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::from_secs(86_400),
            vec![],
            vec![],
        )
    }
}
