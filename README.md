# Parallel Integration Engine - PIE

A Rust application for submitting mathematical computations to an asynchronous job queue. The current implementation includes an Axum HTTP API, a concurrent worker, PostgreSQL persistence, and an interactive terminal client. It runs locally with Docker Compose and can optionally use Amazon SQS instead of the built-in PostgreSQL queue.

## Run the application

The quickest way to start the API, worker, and PostgreSQL database is:

```powershell
docker compose up -d --build
```

The API is then available at `http://localhost:3000`. Check that it is ready with:

```powershell
curl.exe http://localhost:3000/ready
```

To open the interactive terminal client, install a current Rust toolchain and run this from the repository root:

```powershell
cargo run --manifest-path math/Cargo.toml
```

The client connects to `http://localhost:3000` by default. To use a different API URL:

```powershell
$env:MATH_API_URL = "http://localhost:3000"
cargo run --manifest-path math/Cargo.toml
```

Stop the services with `docker compose down`. PostgreSQL data is retained in the `postgres_data` Docker volume.

## Current functionality

The application currently supports:

- Persisting jobs, results, errors, timestamps, retry counts, and integration partitions in PostgreSQL
- Processing jobs asynchronously with configurable worker concurrency
- Using PostgreSQL as a zero-setup local queue or Amazon SQS as an optional queue backend
- Retrying failed work and optionally sending exhausted SQS jobs to a dead-letter queue
- Gracefully stopping worker polling and waiting for active worker tasks to exit
- Reporting aggregate job metrics and per-partition integration progress
- Creating and inspecting jobs through either HTTP or the terminal client

There is no browser-based frontend or committed AWS infrastructure yet. S3 result storage, ECS/RDS deployment, authentication, scheduling, autoscaling, and monitoring integrations remain future work.

## Mathematical tasks

### Monte Carlo pi estimation

Submit a number of random points and receive an estimate of pi:

```powershell
$body = @{
  task_type = "monte_carlo_pi"
  input = @{ iterations = 1000000 }
} | ConvertTo-Json

Invoke-RestMethod -Method Post -Uri http://localhost:3000/jobs `
  -ContentType "application/json" -Body $body
```

A completed result has the form:

```json
{
  "pi_estimate": 3.14159
}
```

### Monte Carlo integration

Integration supports one to ten dimensions and divides a request into independently queued child jobs so multiple workers can process it concurrently. Sampling is deterministic for a given seed, including across partition retries and different worker schedules.

```powershell
$body = @{
  task_type = "monte_carlo_integration"
  input = @{
    expression = "exp(-(x^2 + y^2))"
    variables = @("x", "y")
    bounds = @(
      @{ min = 0.0; max = 1.0 },
      @{ min = 0.0; max = 1.0 }
    )
    samples = 100000
    seed = 42
    partitions = 8
  }
} | ConvertTo-Json -Depth 5

Invoke-RestMethod -Method Post -Uri http://localhost:3000/jobs `
  -ContentType "application/json" -Body $body
```

Supported functions are `sin`, `cos`, `tan`, `exp`, `ln`, `sqrt`, and `abs`. Requests may contain 2 to 100,000,000 samples and 1 to 1,024 partitions; the partition count cannot exceed the sample count. If omitted, `partitions` defaults to 8.

The result includes the estimate, standard error, approximate 95% confidence interval, sample count, and seed.

## Terminal client

The `math` crate provides a full-screen terminal interface and restores the previous terminal screen when it exits. Its main commands run immediately without pressing Enter:

- `t`: list or refresh jobs and metrics
- `r`: open the guided task submission form
- `h` or `?`: show help
- `Esc`: return or cancel
- `q` or `Ctrl+C`: quit

From the task list, use the arrow keys to change pages and the displayed number key to open a job. Integration details include partition and sample progress.

## HTTP API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | Process liveness check |
| `GET` | `/ready` | Readiness check, including PostgreSQL connectivity |
| `POST` | `/jobs` | Validate, persist, and enqueue a job |
| `GET` | `/jobs` | List jobs, newest first |
| `GET` | `/jobs/{id}` | Get a job and, when applicable, partition progress |
| `DELETE` | `/jobs` | Delete all jobs |
| `GET` | `/metrics` | Get pending, running, completed, failed, and total counts |
| `POST` | `/jobs/failures` | Create an intentionally failing job for retry/DLQ testing |
| `POST` | `/jobs/{id}/run` | Run a pending job directly; intended for testing |

Job statuses are `PENDING`, `RUNNING`, `COMPLETED`, and `FAILED`. The create endpoint returns a job ID:

```json
{
  "job_id": "ed5d7b77-4c9f-463a-a0eb-8be49e054af5"
}
```

## Local development

Prerequisites are Rust, Docker, and Docker Compose. Start only PostgreSQL:

```powershell
docker compose up -d postgres
```

Copy `api/.env.example` to `api/.env`, then set `JOB_QUEUE_BACKEND=postgres` for local development. In separate terminals, run:

```powershell
cd api
cargo run --bin api
```

```powershell
cd api
cargo run --bin worker
```

Both binaries run SQLx migrations automatically at startup. Useful worker settings are:

| Variable | Default | Description |
| --- | ---: | --- |
| `WORKER_MAX_RETRIES` | `3` | Failed attempts before a job becomes failed |
| `WORKER_POLL_INTERVAL_SECONDS` | `1` | Delay between queue polls |
| `WORKER_CONCURRENCY` | `1` | Concurrent worker loops in one process |

Run the test suites with:

```powershell
cargo test --manifest-path api/Cargo.toml
cargo test --manifest-path math/Cargo.toml
```

Repository layout:

```text
api/                  Axum API, worker, queues, database access, and task runners
api/migrations/       PostgreSQL schema migrations
math/                 Interactive terminal client
docker-compose.yml    Local PostgreSQL, API, and worker services
```

## Amazon SQS queue backend

The API and worker must use the same backend and queue configuration:

```text
JOB_QUEUE_BACKEND=sqs
AWS_REGION=us-east-2
SQS_QUEUE_URL=https://sqs.us-east-2.amazonaws.com/<account-id>/task-queue
SQS_DLQ_URL=https://sqs.us-east-2.amazonaws.com/<account-id>/task-queue-dlq
```

AWS credentials are loaded through the standard AWS SDK credential chain. `SQS_DLQ_URL` is optional. When it is set, the worker marks a job failed on the `WORKER_MAX_RETRIES` attempt, sends its message to the dead-letter queue, and removes it from the main queue.

If the main queue also has an SQS redrive policy, configure `maxReceiveCount` to be at least `WORKER_MAX_RETRIES` so the worker can record the final failure in PostgreSQL before SQS moves the message.
