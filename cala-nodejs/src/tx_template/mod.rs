mod values;

use cala_ledger::velocity::NewParamDefinition;
use cala_types::param::ParamDataType;
use values::*;

#[napi]
pub struct CalaTxTemplates {
  inner: cala_ledger::tx_template::TxTemplates,
}

#[napi]
pub struct CalaTxTemplate {
  inner: cala_ledger::tx_template::TxTemplate,
}

#[napi]
impl CalaTxTemplate {
  #[napi]
  pub fn id(&self) -> String {
    self.inner.id().to_string()
  }

  #[napi]
  pub fn values(&self) -> TxTemplateValues {
    TxTemplateValues::from(&self.inner)
  }
}

#[napi]
impl CalaTxTemplates {
  pub fn new(inner: &cala_ledger::tx_template::TxTemplates) -> Self {
    Self {
      inner: inner.clone(),
    }
  }

  #[napi]
  pub async fn find_by_code(&self, code: String) -> napi::Result<CalaTxTemplate> {
    let template = self
      .inner
      .find_by_code(code)
      .await
      .map_err(crate::generic_napi_error)?;

    Ok(CalaTxTemplate { inner: template })
  }

  #[napi]
  pub async fn create(&self, new_tx_template: NewTxTemplateValues) -> napi::Result<CalaTxTemplate> {
    let id = if let Some(id) = new_tx_template.id {
      id.parse::<cala_ledger::TxTemplateId>()
        .map_err(crate::generic_napi_error)?
    } else {
      cala_ledger::TxTemplateId::new()
    };

    let mut tx_template_params = Vec::new();

    let mut new = cala_ledger::tx_template::NewTxTemplate::builder();

    if let Some(params) = new_tx_template.params {
      for param in params {
        let param_type = match param.r#type {
          ParamDataTypeValues::String => ParamDataType::String,
          ParamDataTypeValues::Integer => ParamDataType::Integer,
          ParamDataTypeValues::Decimal => ParamDataType::Decimal,
          ParamDataTypeValues::Boolean => ParamDataType::Boolean,
          ParamDataTypeValues::Uuid => ParamDataType::Uuid,
          ParamDataTypeValues::Date => ParamDataType::Date,
          ParamDataTypeValues::Timestamp => ParamDataType::Timestamp,
          ParamDataTypeValues::Json => ParamDataType::Json,
        };
        let mut param_builder = NewParamDefinition::builder();
        param_builder.name(param.name).r#type(param_type);

        tx_template_params.push(param_builder.build().map_err(crate::generic_napi_error)?);
      }

      new.params(tx_template_params.clone());
    }

    new.id(id).code(new_tx_template.code);
    if let Some(description) = new_tx_template.description {
      new.description(description);
    }

    if let Some(transaction) = new_tx_template.transaction {
      let mut new_transaction =
        cala_ledger::tx_template::NewTxTemplateTransactionBuilder::default();

      new_transaction.effective(transaction.effective);
      new_transaction.journal_id(transaction.journal_id);

      if let Some(correlation_id) = transaction.correlation_id {
        new_transaction.correlation_id(correlation_id);
      }

      if let Some(external_id) = transaction.external_id {
        new_transaction.external_id(external_id);
      }

      if let Some(description) = transaction.description {
        new_transaction.description(description);
      }

      if let Some(metadata) = transaction.metadata {
        new_transaction.metadata(metadata);
      }

      new.transaction(new_transaction.build().map_err(crate::generic_napi_error)?);
    } else {
      return Err(napi::Error::from_reason(
        "Transaction details are required".to_string(),
      ));
    }

    if let Some(metadata) = new_tx_template.metadata {
      let _ = new.metadata(metadata);
    }

    let mut tx_template_entries = Vec::new();

    if new_tx_template.entries.is_empty() {
      return Err(napi::Error::from_reason(
        "At least one entry is required".to_string(),
      ));
    }

    for entry in new_tx_template.entries {
      let mut entry_builder = cala_ledger::tx_template::NewTxTemplateEntry::builder();

      entry_builder
        .entry_type(entry.entry_type)
        .account_id(entry.account_id)
        .layer(entry.layer)
        .direction(entry.direction)
        .units(entry.units)
        .currency(entry.currency);

      if let Some(description) = entry.description {
        entry_builder.description(description);
      }

      if let Some(metadata) = entry.metadata {
        entry_builder.metadata(metadata);
      }

      tx_template_entries.push(entry_builder.build().map_err(crate::generic_napi_error)?);
    }

    new.entries(tx_template_entries.clone());

    let tx_template = self
      .inner
      .create(new.build().map_err(crate::generic_napi_error)?)
      .await
      .map_err(crate::generic_napi_error)?;
    Ok(CalaTxTemplate { inner: tx_template })
  }
}
