# Agent Integration Tests

Verify that AI agents, given patchloom's AGENTS.md instructions, actually use patchloom for file operations instead of raw tools.

## Prerequisites

- Python 3.10+
- `patchloom` binary (run `cargo build` first)
- `grok` CLI installed and configured (or another supported agent)
- `jq` installed (used by the patchloom shim)
- API key set: `export GROK_CODE_XAI_API_KEY="xai-..."`

## Running tests

```bash
# Run all tests with default agent (grok) and model (grok-build)
make agent-test

# Or run directly with options
cd tests/agent
pip install -r requirements.txt
pytest -v --agent grok --model grok-build

# Run a specific test
pytest -v -k test_search

# Use a different model
pytest -v --model gpt-5
```

## How it works

1. Each test creates an isolated temp directory with an `AGENTS.md` generated from `patchloom agent-rules`
2. Fixture files are written for the scenario
3. A **patchloom shim** wraps the real binary and logs every invocation to a JSONL file
4. The agent is invoked in headless mode with the shim on PATH
5. After the agent completes, the test asserts:
   - **Patchloom was used** for the expected command (primary, hard failure)
   - **Correct file state** after the operation (secondary)

## Adding a new scenario

1. Pick the right test file (`test_basic.py`, `test_batch.py`, `test_structured.py`, or `test_files.py`)
2. Write fixture files into the `workspace`
3. Call `run_scenario(agent, workspace, patchloom_shim, prompt)`
4. Assert with `assert_patchloom_used(result, "command")`
5. Verify file state

## Adding a new agent driver

1. Create `drivers/myagent.py` implementing `AgentDriver`
2. Register it in `drivers/base.py` `create_driver()`
3. Run: `pytest -v --agent myagent --model my-model`

## Environment variables

| Variable | Description |
|----------|-------------|
| `AGENT_TEST_AGENT` | Agent name (default: `grok`) |
| `AGENT_TEST_MODEL` | Model name (default: `grok-build`) |
| `GROK_CODE_XAI_API_KEY` | API key for Grok Build CLI |
