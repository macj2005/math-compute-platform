RUN COMMANDS:

docker compose up -d --build (run everything)
  OR
docker compose up -d postgres (just db)
cargo run --bin api
cargo run --bin worker

SQS CONFIG:

JOB_QUEUE_BACKEND=sqs
SQS_QUEUE_URL=https://sqs.<region>.amazonaws.com/<account-id>/task-queue
SQS_DLQ_URL=https://sqs.<region>.amazonaws.com/<account-id>/task-queue-dlq

When SQS_DLQ_URL is set, the worker marks jobs as failed on the
WORKER_MAX_RETRIES failed attempt, sends them to the dead-letter queue, and
deletes the message from the main queue. If the SQS queue also has a native
redrive policy, set its maxReceiveCount to the same value or higher than
WORKER_MAX_RETRIES so Postgres is updated before SQS moves the message.



Here is the summary of the project we are going to build:
Project: Distributed Mathematical Task Queue (Rust + AWS)

Build a cloud-native distributed task processing platform similar to Celery, AWS Batch, or Sidekiq.

Goal

Users submit long-running computational jobs through a web interface. Jobs are processed asynchronously by a fleet of Rust workers running in AWS.

The system should demonstrate:

Distributed systems
Queue-based architecture
Rust async programming
AWS cloud services
Horizontal scaling
Fault tolerance
Monitoring

The project should be production-oriented and suitable for a software engineering resume.

High-Level Architecture
Frontend (Next.js)

        ↓

Rust API Service (Axum)

        ↓

Amazon SQS Queue

        ↓

Rust Worker Fleet

        ↓

PostgreSQL + S3

        ↓

Frontend Dashboard

Workflow:

User submits a job.
API creates a job record in PostgreSQL.
API sends a message to SQS.
Worker pulls message from SQS.
Worker executes task.
Worker stores result.
User sees status updates in dashboard.

The system should follow the work queue pattern where producers create jobs and workers consume jobs asynchronously. SQS should be the message broker.

Tech Stack

Backend
Rust
Tokio
Axum
SQLx
Serde
UUID
Tracing

Frontend
Next.js
TypeScript
Tailwind
Database
PostgreSQL

AWS
Amazon SQS
Amazon ECS
Amazon ECR
Amazon RDS
Amazon S3
Amazon CloudWatch
IAM

Job Types

The platform is a distributed mathematical computing system.

Implement:

Monte Carlo Pi Estimation

Input:

{
"iterations": 10000000
}

Output:

{
"pi_estimate": 3.14159
}
I might add more job types later.

Database Schema

Create a jobs table:

jobs
-----
id UUID
task_type TEXT
status TEXT
input JSONB
result JSONB
error TEXT
created_at TIMESTAMP
started_at TIMESTAMP
completed_at TIMESTAMP
retry_count INTEGER
API Endpoints
Create Job
POST /jobs

Creates a new job and places a message into SQS.

Returns:

{
"job_id": "uuid"
}
Get Job
GET /jobs/{id}

Returns:

{
"status": "RUNNING",
"progress": 50
}
List Jobs
GET /jobs

Returns recent jobs.

Dashboard Metrics
GET /metrics

Returns:

{
"pending_jobs": 12,
"running_jobs": 4,
"completed_today": 400,
"failed_today": 3
}
Worker Service

Create a separate Rust service.

Responsibilities:

Poll SQS.
Receive messages.
Mark job RUNNING.
Execute task.
Store result.
Delete message from queue.

Use Tokio async tasks.

Support multiple concurrent jobs.

Implement graceful shutdown.

AWS Infrastructure
SQS

Create:

Main Queue
task-queue

Stores incoming jobs.

Dead Letter Queue
task-queue-dlq

Stores failed jobs after max retries.

Implement visibility timeouts and retry logic. SQS supports durable queues, retries, visibility timeouts, and dead-letter queues for failed messages.

RDS

Use PostgreSQL for:

Job metadata
Status tracking
Results
S3

Store:

Large reports
CSV exports
Generated files

Database should only store S3 URLs.

ECS

Deploy:

API Service
api-service
Worker Service
worker-service

Each service should run as Docker containers.

ECR

Store Docker images.

CloudWatch

Track:

Queue depth
Jobs completed
Jobs failed
Worker errors
Average execution time
Frontend Requirements

Build a dashboard.

Home Page

Show:

Total jobs
Running jobs
Failed jobs
Completed jobs
Create Job Page

Form:

Task Type
Parameters
Submit Button
Job Details Page

Show:

Job ID
Status
Progress
Result
Error Messages
Execution Time
Admin Dashboard

Show:

Queue Depth
Workers Online
Completed Jobs
Failed Jobs
Average Runtime
Stretch Goals

Implement:

JWT authentication
Job priorities
Scheduled jobs
Auto-scaling workers
WebSocket live updates
Terraform infrastructure
Prometheus metrics
Grafana dashboards
Worker heartbeats
Duplicate job detection
FIFO queue support
Resume Goal

The finished project should demonstrate:

Rust
Tokio
Distributed systems
AWS SQS
AWS ECS
PostgreSQL
Cloud architecture
Fault tolerance
Async job processing
System design

The final architecture should resemble a simplified production job-processing platform used in large-scale cloud systems.
