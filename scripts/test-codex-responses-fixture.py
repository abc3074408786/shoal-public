#!/usr/bin/env python3
"""Repeatable local HTTP tests; no Codex process, account, or model is involved."""
import json
from pathlib import Path
import runpy
import tempfile
import unittest
import urllib.error
import urllib.request


FIXTURE = Path(__file__).resolve().parent / "fixtures/codex-responses-server.py"
Server = runpy.run_path(str(FIXTURE))["Server"]
TOOL = {
    "type": "function", "name": "exec_command",
    "parameters": {
        "type": "object", "properties": {"cmd": {"type": "string"}},
        "required": ["cmd"], "additionalProperties": False,
    },
}


class ResponsesFixtureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="vl-responses-unit-")
        self.addCleanup(self.temporary.cleanup)
        self.output = Path(self.temporary.name) / "evidence"
        # Deliberately avoid inheriting HTTP(S)_PROXY for localhost tests.
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def steps(self, expected=None):
        tool = {"kind": "tool", "name": "exec_command",
                "arguments": {"cmd": "printf fixed"}}
        if expected is not None:
            tool["expect_output_contains"] = expected
        return [tool, {"kind": "final", "text": "Fixed test complete."}]

    def body(self, tools=None, output=None):
        inputs = [] if output is None else [{
            "type": "function_call_output", "call_id": "fixture-1", "output": output,
        }]
        return {"stream": True, "tools": [TOOL] if tools is None else tools, "input": inputs}

    def request(self, server, body=None, *, path="/responses", method="POST", headers=None, raw=None):
        encoded = raw if raw is not None else json.dumps(body or self.body()).encode()
        request = urllib.request.Request(
            server.base_url + path, data=encoded if method == "POST" else None,
            headers={"Content-Type": "application/json", **(headers or {})}, method=method,
        )
        try:
            with self.http.open(request, timeout=3) as response:
                return response.status, response.read().decode()
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode()

    def receipt(self):
        return json.loads((self.output / "responses-summary.json").read_text())

    def complete_sequence(self, namespace=None, expected=None, output="fixed returned result"):
        tools = [TOOL] if namespace is None else [{
            "type": "namespace", "name": namespace, "tools": [TOOL],
        }]
        with Server(self.steps(expected), self.output, timeout=5) as server:
            self.assertEqual(server.base_url, f"http://127.0.0.1:{server.port}/v1")
            code, response = self.request(server, self.body(tools))
            self.assertEqual(code, 200)
            events = [json.loads(line[6:]) for line in response.splitlines() if line.startswith("data: ")]
            self.assertEqual([event["type"] for event in events], [
                "response.created", "response.output_item.done", "response.completed",
            ])
            item = events[1]["item"]
            self.assertEqual(item["name"], "exec_command")
            self.assertEqual(item["call_id"], "fixture-1")
            self.assertEqual(item.get("namespace"), namespace)
            self.assertEqual(json.loads(item["arguments"]), {"cmd": "printf fixed"})
            self.assertEqual(events[-1]["response"]["output"], [item])
            self.assertEqual(events[-1]["response"]["status"], "completed")
            code, response = self.request(server, self.body(tools, output))
            self.assertEqual(code, 200)
            self.assertIn("Fixed test complete.", response)
            self.assertTrue(server.assert_complete()["complete"])
            self.assertTrue(all(server.assertions.values()))
        saved = self.receipt()
        self.assertTrue(saved["complete"])
        self.assertEqual(saved["request_count"], 2)
        self.assertNotIn(output, json.dumps(saved))
        self.assertFalse(server.worker.is_alive())
        return saved

    def test_flat_tool_sequence_and_safe_evidence(self):
        self.complete_sequence()

    def test_namespaced_tool_sequence(self):
        saved = self.complete_sequence("functions")
        self.assertEqual(saved["requests"][0]["tool_names"], ["functions.exec_command"])
        self.assertEqual(saved["requests"][0]["selected_namespace"], "functions")

    def test_string_output_marker(self):
        saved = self.complete_sequence(expected="kernel passed", output="kernel passed; fixed")
        self.assertEqual(saved["requests"][-1]["output_expectations_checked"], 1)
        self.assertTrue(saved["requests"][-1]["output_expectations_passed"])

    def test_multiple_output_markers(self):
        saved = self.complete_sequence(expected=["kernel passed", "outside denied"],
                                       output="kernel passed; outside denied")
        self.assertEqual(saved["requests"][-1]["output_expectations_checked"], 2)
        self.assertTrue(saved["requests"][-1]["output_expectations_passed"])

    def test_two_turns_preserve_unique_calls_and_output_expectations(self):
        steps = self.steps("first tool passed") + self.steps(["second tool passed", "exec-server"])
        steps[1]["text"] = "First turn complete."
        steps[3]["text"] = "Second turn complete."
        with Server(steps, self.output, timeout=5) as server:
            code, first = self.request(server)
            self.assertEqual(code, 200)
            self.assertIn('"call_id":"fixture-1"', first)
            code, first_final = self.request(server, self.body(output="first tool passed"))
            self.assertEqual(code, 200)
            self.assertIn("First turn complete.", first_final)
            self.assertFalse(server.summary()["complete"])
            code, second = self.request(server)
            self.assertEqual(code, 200)
            self.assertIn('"call_id":"fixture-3"', second)
            output = self.body(output="second tool passed; exec-server")
            output["input"][0]["call_id"] = "fixture-3"
            code, second_final = self.request(server, output)
            self.assertEqual(code, 200)
            self.assertIn("Second turn complete.", second_final)
            self.assertTrue(server.assert_complete()["complete"])
            self.assertTrue(all(server.assertions.values()))
        saved = self.receipt()
        self.assertEqual(saved["request_count"], 4)
        self.assertEqual([record["returned_call_id"] for record in saved["requests"]
                          if "returned_call_id" in record], ["fixture-1", "fixture-3"])
        self.assertEqual([record["output_expectations_checked"] for record in saved["requests"]
                          if "output_expectations_checked" in record], [1, 2])
        self.assertTrue(all(record["output_expectations_passed"] for record in saved["requests"]
                            if "output_expectations_passed" in record))

    def test_last_step_must_remain_final_with_multiple_turns(self):
        for steps in (self.steps()[:-1], self.steps() + self.steps()[:-1]):
            with self.subTest(step_count=len(steps)), self.assertRaisesRegex(ValueError, "last step must be final"):
                Server(steps, self.output)

    def test_missing_output_prevents_final_response(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 200)
            self.assertEqual(self.request(server)[0], 400)
            with self.assertRaises(AssertionError):
                server.assert_complete()
        self.assertEqual(self.receipt()["completed_steps"], 1)

    def test_wrong_call_id_prevents_final_response(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 200)
            body = self.body(output="result")
            body["input"][0]["call_id"] = "different-call"
            self.assertEqual(self.request(server, body)[0], 400)
        self.assertFalse(self.receipt()["complete"])

    def test_duplicate_output_prevents_final_response(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 200)
            body = self.body(output="result")
            body["input"] *= 2
            self.assertEqual(self.request(server, body)[0], 400)
        self.assertFalse(self.receipt()["complete"])

    def test_missing_marker_prevents_final_response(self):
        with Server(self.steps(["passed", "outside denied"]), self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 200)
            self.assertEqual(self.request(server, self.body(output="passed"))[0], 400)
        saved = self.receipt()
        self.assertFalse(saved["complete"])
        self.assertFalse(saved["requests"][-1]["output_expectations_passed"])

    def test_empty_output_prevents_final_response(self):
        with Server(self.steps("passed"), self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 200)
            self.assertEqual(self.request(server, self.body(output=""))[0], 400)
        self.assertFalse(self.receipt()["requests"][-1]["output_expectations_passed"])

    def test_tool_substitution_rejected(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            body = self.body([dict(TOOL, name="shell")])
            self.assertEqual(self.request(server, body)[0], 400)
        self.assertEqual(self.receipt()["completed_steps"], 0)

    def test_ambiguous_tool_rejected(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server, self.body([TOOL, TOOL]))[0], 400)
        self.assertFalse(self.receipt()["complete"])

    def test_unadvertised_fixed_argument_rejected(self):
        steps = self.steps()
        steps[0]["arguments"]["unadvertised"] = True
        with Server(steps, self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 400)
        self.assertFalse(self.receipt()["complete"])

    def test_wrong_argument_type_rejected(self):
        steps = self.steps()
        steps[0]["arguments"]["cmd"] = ["not", "a", "string"]
        with Server(steps, self.output, timeout=5) as server:
            self.assertEqual(self.request(server)[0], 400)
        self.assertFalse(self.receipt()["complete"])

    def test_wrong_path_rejected(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server, path="/responses?unexpected=yes")[0], 404)
        self.assertFalse(self.receipt()["complete"])

    def test_wrong_method_rejected(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server, method="GET")[0], 405)
        self.assertFalse(self.receipt()["complete"])

    def test_credentials_rejected_without_recording_value(self):
        secret = "synthetic-never-store-this"
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server, headers={"Authorization": "Bearer " + secret})[0], 400)
        saved = self.receipt()
        self.assertFalse(saved["complete"])
        self.assertNotIn(secret, json.dumps(saved))

    def test_oversized_body_rejected(self):
        with Server(self.steps(), self.output, timeout=5, max_body=32) as server:
            self.assertEqual(self.request(server)[0], 413)
        self.assertFalse(self.receipt()["complete"])

    def test_malformed_json_rejected(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server, raw=b"{broken")[0], 400)
        self.assertFalse(self.receipt()["complete"])

    def test_compressed_request_rejected(self):
        with Server(self.steps(), self.output, timeout=5) as server:
            self.assertEqual(self.request(server, headers={"Content-Encoding": "zstd"})[0], 415)
        self.assertFalse(self.receipt()["complete"])

    def test_old_evidence_not_overwritten(self):
        self.output.mkdir()
        receipt = self.output / "responses-summary.json"
        receipt.write_text("existing evidence\n")
        with self.assertRaises(ValueError):
            Server(self.steps(), self.output)
        self.assertEqual(receipt.read_text(), "existing evidence\n")

    def test_invalid_output_expectation_rejected(self):
        for expected in ("", [""], ["valid", 42], {"not": "a string"}):
            with self.subTest(expected=expected), self.assertRaises(ValueError):
                Server(self.steps(expected), self.output)


if __name__ == "__main__":
    unittest.main()
