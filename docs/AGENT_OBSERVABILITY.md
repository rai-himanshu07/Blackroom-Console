# Agent Observability

Measure agent behavior before adding more instructions, tools, or agents. The
goal is to identify context, cache, cost, and reliability problems without
recording source code or prompts in another system.

## VS Code

- The context control in chat shows current token use and cumulative session AI
  credits. Hover a response for per-turn cost.
- Open **Developer: Open Agent Debug Panel** for tool calls, durations, request
  composition, and customization discovery.
- Use **Cache Explorer** to find the first prompt-prefix divergence when cache
  hit rates fall. Common causes are model, reasoning, tool-set, instruction, and
  early-context changes.
- Use `/compact` for a focused summary when a useful session grows; start a new
  session when the task changes.
- Use `/fork` or a checkpoint fork for alternatives and side questions rather
  than contaminating the main implementation thread.

## Copilot CLI

Useful built-in commands:

```text
/context
/usage
/env
/tasks
/chronicle tips
/chronicle cost-tips
```

Set a soft per-response credit ceiling before autonomous work:

```text
/limits set max-ai-credits VALUE
```

In programmatic use, the equivalent is `--max-ai-credits`. A response limit is
not a monthly budget and resets for each user message.

## OpenTelemetry

Copilot CLI and supported VS Code agent sessions can export OpenTelemetry traces
and metrics. Metadata-only telemetry can answer:

- which models and agents consume tokens;
- cache-read and cache-creation token counts;
- turns and tool calls per request;
- tool latency and failure rates;
- compaction, truncation, and session-abort events.

OTel is off by default. Enable it only to a trusted local file or approved
collector, apply retention and access controls, and document who owns the data.

**Do not enable full content capture by default.** In particular, leave
`OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` unset or false. Enabling it
can export prompts, responses, system instructions, tool definitions, file
contents, command arguments, and tool results, including secrets and proprietary
code.

## Evaluation

Operational telemetry measures cost and behavior, not correctness. Pair it with
deterministic tests and, for customization quality, reviewed evaluation cases.
VS Code's optional Chat Customizations Evaluations extension and Waza can test
skill behavior, but model-scored results remain advisory unless a human-defined
acceptance threshold and stable test corpus make them actionable.
