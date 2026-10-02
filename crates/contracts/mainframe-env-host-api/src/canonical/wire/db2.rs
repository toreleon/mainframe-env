use super::*;

impl Canonical for Db2Operation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::ExecuteScript => out.variant("Db2Operation", "ExecuteScript", 0),
            Self::FreePlans => out.variant("Db2Operation", "FreePlans", 0),
            Self::Select => out.variant("Db2Operation", "Select", 0),
            Self::Insert => out.variant("Db2Operation", "Insert", 0),
            Self::Update => out.variant("Db2Operation", "Update", 0),
            Self::Delete => out.variant("Db2Operation", "Delete", 0),
            Self::Count => out.variant("Db2Operation", "Count", 0),
            Self::DeclareCursor => out.variant("Db2Operation", "DeclareCursor", 0),
            Self::OpenCursor => out.variant("Db2Operation", "OpenCursor", 0),
            Self::FetchCursor => out.variant("Db2Operation", "FetchCursor", 0),
            Self::CloseCursor => out.variant("Db2Operation", "CloseCursor", 0),
            Self::Commit => out.variant("Db2Operation", "Commit", 0),
            Self::Rollback => out.variant("Db2Operation", "Rollback", 0),
            Self::Extract => out.variant("Db2Operation", "Extract", 0),
        }
    }
}

impl Canonical for Db2HostVariable {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { indicator, value } = self;
        out.object("Db2HostVariable", 2)?;
        out.text("indicator")?;
        indicator.encode(out)?;
        out.text("value")?;
        value.encode(out)?;
        Ok(())
    }
}

impl Canonical for Db2Request {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            cursor,
            inputs,
            max_rows,
            mutation,
            operation,
            outputs,
            statement,
        } = self;
        out.object("Db2Request", 7)?;
        out.text("cursor")?;
        cursor.encode(out)?;
        out.text("inputs")?;
        inputs.encode(out)?;
        out.text("max_rows")?;
        max_rows.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("operation")?;
        operation.encode(out)?;
        out.text("outputs")?;
        outputs.encode(out)?;
        out.text("statement")?;
        statement.encode(out)?;
        Ok(())
    }
}

impl Canonical for Db2Row {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { columns } = self;
        out.object("Db2Row", 1)?;
        out.text("columns")?;
        columns.encode(out)?;
        Ok(())
    }
}

impl Canonical for Db2Result {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            affected_rows,
            message,
            rows,
            sqlcode,
            sqlstate,
        } = self;
        out.object("Db2Result", 5)?;
        out.text("affected_rows")?;
        affected_rows.encode(out)?;
        out.text("message")?;
        message.encode(out)?;
        out.text("rows")?;
        rows.encode(out)?;
        out.text("sqlcode")?;
        sqlcode.encode(out)?;
        out.text("sqlstate")?;
        sqlstate.encode(out)?;
        Ok(())
    }
}
