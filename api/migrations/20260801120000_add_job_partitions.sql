ALTER TABLE jobs
    ADD COLUMN parent_job_id UUID REFERENCES jobs(id) ON DELETE CASCADE,
    ADD COLUMN partition_index INTEGER;

ALTER TABLE jobs
    ADD CONSTRAINT job_partition_identity_complete CHECK (
        (parent_job_id IS NULL AND partition_index IS NULL)
        OR (parent_job_id IS NOT NULL AND partition_index IS NOT NULL AND partition_index >= 0)
    ),
    ADD CONSTRAINT job_partition_identity_unique UNIQUE (parent_job_id, partition_index);

CREATE INDEX idx_jobs_parent_job_id ON jobs (parent_job_id);
