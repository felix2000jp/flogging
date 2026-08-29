mod jira;
mod ollama;

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;

use crate::suggestions::SuggestionSet;

pub struct SuggestionAgent {
    result_receiver: Option<Receiver<Result<AgentResult>>>,
}

impl SuggestionAgent {
    pub fn new() -> Self {
        Self {
            result_receiver: None,
        }
    }

    pub fn start(&mut self, request: AgentRequest) -> Result<()> {
        if self.result_receiver.is_some() {
            return Err(anyhow!("a suggestion job is already running"));
        }

        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("suggestion-agent".to_owned())
            .spawn(move || {
                let result = ollama::generate_suggestions(request);
                let _ = result_sender.send(result);
            })
            .context("could not start the suggestion agent thread")?;
        self.result_receiver = Some(result_receiver);

        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.result_receiver.is_some()
    }

    pub fn try_finish(&mut self) -> Option<Result<AgentResult>> {
        let result = match self.result_receiver.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(anyhow!(
                "suggestion agent stopped without returning a result"
            )),
        };

        self.result_receiver = None;
        Some(result)
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
