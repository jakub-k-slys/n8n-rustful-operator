use crate::{Error, Result, reconciler::validate::validate_database, spec::Single};

pub fn validate_single(s: &Single) -> Result<()> {
    if s.metadata.name.as_deref() == Some("illegal") {
        return Err(Error::IllegalSingle);
    }
    if let Some(net) = &s.spec.networking
        && net.ingress.is_some()
        && net.http_route.is_some()
    {
        return Err(Error::ConflictingNetworking);
    }
    if let Some(db) = &s.spec.database {
        validate_database(db)?;
    }
    Ok(())
}
