pub trait IncludeMigrations {
    fn include_cala_migrations(&mut self) -> &Self;
}

impl IncludeMigrations for sqlx::migrate::Migrator {
    fn include_cala_migrations(&mut self) -> &Self {
        let mut new_migrations = self.migrations.to_vec();
        new_migrations.extend_from_slice(&sqlx::migrate!().migrations);

        self.migrations = std::borrow::Cow::Owned(new_migrations);

        self
    }
}
