use std::io;
use std::ops::Range;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    api::{ApiClient, Job, JobDetails, Metrics},
    details::detail_lines,
    terminal,
    wizard::{
        RequestWizard, TaskKind, build_request, field_count, field_default, field_help,
        field_label, is_final_field, task_label, validate_field,
    },
};

const TASKS_PER_PAGE: usize = 10;

enum Screen {
    Home,
    Tasks,
    Details,
    RequestSelect,
    RequestField,
    Help,
}

pub(crate) struct App {
    api: ApiClient,
    screen: Screen,
    jobs: Vec<Job>,
    task_page: usize,
    metrics: Option<Metrics>,
    details: Option<JobDetails>,
    detail_scroll: usize,
    request: RequestWizard,
    message: String,
}

impl App {
    pub(crate) fn new(api_url: String) -> Self {
        Self {
            api: ApiClient::new(api_url),
            screen: Screen::Home,
            jobs: Vec::new(),
            task_page: 0,
            metrics: None,
            details: None,
            detail_scroll: 0,
            request: RequestWizard::new(),
            message: "Ready".to_owned(),
        }
    }

    pub(crate) async fn run(&mut self) -> io::Result<()> {
        loop {
            self.draw()?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if is_ctrl_c(key) || self.handle_key(key).await? {
                return Ok(());
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        if matches!(self.screen, Screen::Tasks) {
            if let KeyCode::Char(selector @ '0'..='9') = key.code {
                self.load_task_details(selector.to_digit(10).unwrap() as usize)
                    .await?;
                return Ok(false);
            }
            if self.handle_task_page_key(key) {
                return Ok(false);
            }
        }
        match self.screen {
            Screen::RequestSelect => self.handle_task_selection(key),
            Screen::RequestField => self.handle_field_input(key).await?,
            Screen::Details => return self.handle_detail_key(key).await,
            _ => match key.code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Char('t') => self.load_tasks().await?,
                KeyCode::Char('r') => self.start_request(),
                KeyCode::Char('h') | KeyCode::Char('?') => self.screen = Screen::Help,
                KeyCode::Esc => self.screen = Screen::Home,
                _ => {}
            },
        }
        Ok(false)
    }

    fn handle_task_page_key(&mut self, key: KeyEvent) -> bool {
        let pages = task_page_count(self.jobs.len());
        match key.code {
            KeyCode::Left | KeyCode::Up if self.task_page > 0 => {
                self.task_page -= 1;
                self.message = format!("Task page {} of {pages}", self.task_page + 1);
                true
            }
            KeyCode::Right | KeyCode::Down if self.task_page + 1 < pages => {
                self.task_page += 1;
                self.message = format!("Task page {} of {pages}", self.task_page + 1);
                true
            }
            KeyCode::Left | KeyCode::Up | KeyCode::Right | KeyCode::Down => true,
            _ => false,
        }
    }

    async fn load_task_details(&mut self, selector: usize) -> io::Result<()> {
        let index = self.task_page * TASKS_PER_PAGE + selector;
        let Some(job_id) = self.jobs.get(index).map(|job| job.id.clone()) else {
            self.message = format!("No task is assigned to [{selector}] on this page");
            return Ok(());
        };

        self.message = format!("Loading task {job_id}...");
        self.draw()?;
        match self.api.job_details(&job_id).await {
            Ok(details) => {
                self.details = Some(details);
                self.detail_scroll = 0;
                self.screen = Screen::Details;
                self.message = "Task details - arrows scroll, Esc returns to list".to_owned();
            }
            Err(error) => self.message = error,
        }
        Ok(())
    }

    async fn handle_detail_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Tasks;
                self.message = "Task list".to_owned();
            }
            KeyCode::Up => self.detail_scroll = self.detail_scroll.saturating_sub(1),
            KeyCode::Down => {
                let visible = crossterm::terminal::size()?.1.saturating_sub(7) as usize;
                let line_count = self
                    .details
                    .as_ref()
                    .map(detail_lines)
                    .map_or(0, |v| v.len());
                let max_scroll = line_count.saturating_sub(visible);
                self.detail_scroll = (self.detail_scroll + 1).min(max_scroll);
            }
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('t') => self.load_tasks().await?,
            _ => {}
        }
        Ok(false)
    }

    fn handle_task_selection(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_request(),
            KeyCode::Char('1') => self.select_task(TaskKind::Pi),
            KeyCode::Char('2') => self.select_task(TaskKind::Integration),
            _ => {}
        }
    }

    async fn handle_field_input(&mut self, key: KeyEvent) -> io::Result<()> {
        match key.code {
            KeyCode::Esc => self.cancel_request(),
            KeyCode::Enter => self.accept_field().await?,
            KeyCode::Backspace => {
                self.request.input.pop();
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.request.input.push(character);
            }
            _ => {}
        }
        Ok(())
    }

    fn start_request(&mut self) {
        self.request = RequestWizard::new();
        self.message = "Press [1] or [2] to select a task".to_owned();
        self.screen = Screen::RequestSelect;
    }

    fn select_task(&mut self, kind: TaskKind) {
        self.request.select(kind);
        self.screen = Screen::RequestField;
        self.message = field_help(kind, 0).to_owned();
    }

    fn cancel_request(&mut self) {
        self.screen = Screen::Home;
        self.message = "Request cancelled".to_owned();
    }

    async fn load_tasks(&mut self) -> io::Result<()> {
        self.message = "Loading tasks and metrics...".to_owned();
        self.draw()?;
        match self.api.tasks_and_metrics().await {
            Ok((mut jobs, metrics)) => {
                jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
                self.jobs = jobs;
                self.metrics = Some(metrics);
                self.task_page = 0;
                self.screen = Screen::Tasks;
                self.message = "Tasks loaded - arrow keys change pages".to_owned();
            }
            Err(error) => self.message = error,
        }
        Ok(())
    }

    async fn accept_field(&mut self) -> io::Result<()> {
        let kind = self.request.kind.expect("request task is selected");
        let value = match self.current_value(kind) {
            Ok(value) => value,
            Err(error) => {
                self.message = error;
                return Ok(());
            }
        };

        if let Err(error) = validate_field(kind, self.request.field, &value, &self.request.values) {
            self.message = error;
            return Ok(());
        }

        self.request.values.push(value);
        self.request.input.clear();
        if !is_final_field(kind, self.request.field) {
            self.request.field += 1;
            self.message = field_help(kind, self.request.field).to_owned();
            return Ok(());
        }
        self.submit_request(kind).await
    }

    fn current_value(&self, kind: TaskKind) -> Result<String, String> {
        let value = self.request.input.trim();
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
        field_default(kind, self.request.field)
            .map(str::to_owned)
            .ok_or_else(|| "This field is required".to_owned())
    }

    async fn submit_request(&mut self, kind: TaskKind) -> io::Result<()> {
        let (task_type, input) = build_request(kind, &self.request.values)
            .expect("wizard fields were validated before submission");
        self.message = "Submitting request...".to_owned();
        self.draw()?;

        match self.api.create_job(task_type, input).await {
            Ok(job_id) => {
                self.screen = Screen::Home;
                self.message = format!("Task submitted: {job_id}");
            }
            Err(error) => self.message = error,
        }
        Ok(())
    }

    fn draw(&self) -> io::Result<()> {
        let content = self.content_lines();
        let cursor =
            matches!(self.screen, Screen::RequestField).then(|| self.request.input.chars().count());
        let scroll = matches!(self.screen, Screen::Details)
            .then_some(self.detail_scroll)
            .unwrap_or(0);
        terminal::draw(&content, &self.message, cursor, scroll)
    }

    fn content_lines(&self) -> Vec<String> {
        match self.screen {
            Screen::Home => home_lines(),
            Screen::Help => help_lines(),
            Screen::RequestSelect => task_selection_lines(),
            Screen::RequestField => self.request_field_lines(),
            Screen::Tasks => self.task_lines(),
            Screen::Details => self
                .details
                .as_ref()
                .map(detail_lines)
                .unwrap_or_else(|| vec!["Task details are unavailable.".to_owned()]),
        }
    }

    fn request_field_lines(&self) -> Vec<String> {
        let kind = self.request.kind.expect("request task is selected");
        vec![
            format!(
                "NEW REQUEST  /  {}  /  FIELD {} OF {}",
                task_label(kind),
                self.request.field + 1,
                field_count(kind)
            ),
            String::new(),
            field_label(kind, self.request.field).to_owned(),
            format!("> {}", self.request.input),
            String::new(),
            field_help(kind, self.request.field).to_owned(),
            field_default(kind, self.request.field)
                .map(|default| format!("Press Enter for default: {default}"))
                .unwrap_or_else(|| "This field is required.".to_owned()),
            String::new(),
            "Press Enter to continue. Esc cancels.".to_owned(),
        ]
    }

    fn task_lines(&self) -> Vec<String> {
        let mut lines = self.metrics_lines();
        if self.jobs.is_empty() {
            lines.push("No tasks found.".to_owned());
        } else {
            for (indicator, job) in self.jobs[task_page_range(self.task_page, self.jobs.len())]
                .iter()
                .enumerate()
            {
                lines.push(format!(
                    "[{indicator}]  {:<36}  {:<24}  {:<10}  {}",
                    job.id, job.task_type, job.status, job.created_at,
                ));
            }
        }
        lines
    }

    fn metrics_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(metrics) = &self.metrics {
            lines.push(format!(
                "METRICS  total {} | pending {} | running {} | completed {} | failed {}",
                metrics.total_jobs,
                metrics.pending_jobs,
                metrics.running_jobs,
                metrics.completed_jobs,
                metrics.failed_jobs
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "TASKS  /  PAGE {} OF {}  /  Use arrow keys to navigate",
            self.task_page + 1,
            task_page_count(self.jobs.len())
        ));
        lines.push(String::new());
        lines.push(format!(
            "{:<4} {:<36}  {:<24}  {:<10}  {}",
            "#", "ID", "TASK", "STATUS", "CREATED"
        ));
        lines.push("-".repeat(103));
        lines
    }
}

pub(crate) fn task_page_count(task_count: usize) -> usize {
    task_count.div_ceil(TASKS_PER_PAGE).max(1)
}

pub(crate) fn task_page_range(page: usize, task_count: usize) -> Range<usize> {
    let start = (page * TASKS_PER_PAGE).min(task_count);
    let end = (start + TASKS_PER_PAGE).min(task_count);
    start..end
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn home_lines() -> Vec<String> {
    vec![
        "Welcome. Choose a command.".to_owned(),
        String::new(),
        "Press [t] to inspect jobs and platform metrics.".to_owned(),
        "Press [r] to submit a mathematical task.".to_owned(),
    ]
}

fn help_lines() -> Vec<String> {
    vec![
        "KEYBOARD COMMANDS".to_owned(),
        String::new(),
        "t     List or refresh tasks and metrics".to_owned(),
        "r     Open the request form".to_owned(),
        "h/?   Show this guide".to_owned(),
        "Esc   Return home or cancel a request".to_owned(),
        "q     Quit (Ctrl+C also quits)".to_owned(),
    ]
}

fn task_selection_lines() -> Vec<String> {
    vec![
        "NEW REQUEST  /  SELECT A TASK".to_owned(),
        String::new(),
        "[1]  Monte Carlo Pi Estimation".to_owned(),
        "     Estimate pi using randomly sampled points.".to_owned(),
        String::new(),
        "[2]  Monte Carlo Integration".to_owned(),
        "     Estimate a custom one- or multi-dimensional integral.".to_owned(),
        String::new(),
        "Press 1 or 2 to continue. Esc cancels.".to_owned(),
    ]
}
