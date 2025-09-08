pub trait IncludeMigrations {
    fn include_cala_migrations(&mut self) -> &Self;
}

impl IncludeMigrations for sqlx::migrate::Migrator {
    fn include_cala_migrations(&mut self) -> &Self {
        {
            use job::IncludeMigrations;
            self.include_job_migrations();
        }

        let mut new_migrations = self.migrations.to_vec();
        new_migrations.extend_from_slice(&sqlx::migrate!().migrations);

        self.migrations = std::borrow::Cow::Owned(new_migrations);

        self
    }
}
