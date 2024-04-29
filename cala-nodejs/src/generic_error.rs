pub(crate) fn generic_napi_error(e: impl std::fmt::Display) -> napi::Error {
  napi::Error::from_reason(e.to_string())
}
