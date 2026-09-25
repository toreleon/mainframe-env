//! SAF-gated operator reply ingress and CICS console-log projection.

use super::{
    ConsoleMessage, GatewayProblem, GatewayResponse, HostProblem, ProductServer, ServiceClass,
    StatusCode, gateway_problem,
};
use serde_json::{Value, json};

impl ProductServer {
    pub(super) fn cics_console_command(
        &self,
        principal: &str,
        console: &str,
        command: String,
    ) -> Result<GatewayResponse, GatewayProblem> {
        let Some(rest) = command
            .strip_prefix("R ")
            .or_else(|| command.strip_prefix("r "))
        else {
            return Err(gateway_problem(HostProblem::Unsupported));
        };
        let (key, reply) = rest
            .split_once(',')
            .ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
        let key = key.trim();
        if !key.starts_with("cics-operator:")
            || key.len() != "cics-operator:".len() + 64
            || reply.len() > 119
        {
            return Err(gateway_problem(HostProblem::Malformed));
        }
        let message = self
            .cics
            .operator_message(key)
            .map_err(gateway_problem)?
            .ok_or_else(|| gateway_problem(HostProblem::NotFound))?;
        let selected = console.to_ascii_uppercase();
        if message
            .console
            .as_ref()
            .is_some_and(|name| name != &selected)
        {
            return Err(gateway_problem(HostProblem::NotFound));
        }
        let invocation = self
            .invocation(
                principal,
                "security:authorize",
                ServiceClass::System,
                &["host.security.authorize"],
            )
            .map_err(gateway_problem)?;
        let tick = self.jes_tick().map_err(gateway_problem)?;
        let run_unit = self
            .cics
            .submit_operator_reply(&invocation, &selected, key, reply.as_bytes(), tick)
            .map_err(gateway_problem)?;
        self.wake_online_run_unit(&run_unit, tick)
            .map_err(gateway_problem)?;
        Ok(GatewayResponse::json(
            StatusCode::OK,
            json!({"cmd-response-key":key,"reply-accepted":true}),
        ))
    }

    pub(super) fn operator_console_items(
        &self,
        direct: &[ConsoleMessage],
    ) -> Result<Value, GatewayProblem> {
        let mut items = direct
            .iter()
            .map(|message| {
                json!({
                    "key":message.key,
                    "console":message.console,
                    "text":String::from_utf8_lossy(&message.text)
                })
            })
            .collect::<Vec<_>>();
        for message in self.cics.operator_messages().map_err(gateway_problem)? {
            let lines = format_operator_lines(&message.text);
            let displayed = lines
                .iter()
                .map(|line| String::from_utf8_lossy(line).into_owned())
                .collect::<Vec<_>>();
            items.push(json!({
                "key":message.key,
                "console":message.console.unwrap_or_else(|| "ROUTED".into()),
                "text":displayed.join("\n"),
                "lines":displayed,
                "routes":message.routes,
                "action":message.action,
                "reply-pending":message.reply_pending
            }));
        }
        Ok(json!({"items":items}))
    }
}

fn format_operator_lines(text: &[u8]) -> Vec<Vec<u8>> {
    if text.len() <= 113 {
        return vec![text.to_vec()];
    }
    let mut remaining = text;
    let mut lines = Vec::new();
    while !remaining.is_empty() && lines.len() < 10 {
        if remaining.len() <= 69 {
            lines.push(remaining.to_vec());
            remaining = &[];
            break;
        }
        let space = remaining[..69].iter().rposition(|byte| *byte == b' ');
        let split = space.filter(|position| *position > 0).unwrap_or(69);
        lines.push(remaining[..split].to_vec());
        remaining = &remaining[split..];
        while remaining.first() == Some(&b' ') {
            remaining = &remaining[1..];
        }
    }
    if remaining.is_empty() {
        lines
    } else {
        vec![text.to_vec()]
    }
}

#[cfg(test)]
mod tests {
    use super::format_operator_lines;

    #[test]
    fn long_operator_text_uses_at_most_ten_space_broken_lines() {
        assert_eq!(format_operator_lines(b"SHORT"), vec![b"SHORT".to_vec()]);
        let long = b"A LONG OPERATOR MESSAGE THAT MUST CROSS THE FIRST SIXTY NINE BYTE LINE AND CONTINUE ON THE NEXT LINE WITHOUT A LEADING SPACE";
        let lines = format_operator_lines(long);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.len() <= 69));
        assert!(lines.iter().skip(1).all(|line| line.first() != Some(&b' ')));
        let worst = vec![b'X'; 690];
        assert_eq!(format_operator_lines(&worst).len(), 10);
    }
}
