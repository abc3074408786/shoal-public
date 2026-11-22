#!/usr/bin/env python3
"""Bounded loopback-only Responses/SSE fixture; never calls a model or forwards HTTP.

Usage from an owned test harness:
    with Server([
        {"kind": "tool", "name": "exec_command", "arguments": {"cmd": "pwd"}},
        {"kind": "final", "text": "Fixed fixture completed."},
    ], output_dir) as fixture:
        # Configure a fresh Codex HOME with base_url=fixture.base_url.
        ...
        fixture.assert_complete()

The protocol follows the rust-v0.153.4 app-server/core test fixtures. Intermediate
final steps can separate turns; tool outputs must still match the preceding call.
Request headers, prompt text, arguments, and tool-result bodies are never persisted.
"""
from copy import deepcopy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time


MAX_BODY = 4 * 1024 * 1024


def _tools(value, namespace=None):
    if not isinstance(value, list) or len(value) > 512:
        raise ValueError("tools must be an array")
    found = []
    for item in value:
        if not isinstance(item, dict):
            raise ValueError("tool must be an object")
        kind, name = item.get("type"), item.get("name")
        if kind == "namespace":
            if not isinstance(name, str) or not name or len(name) > 128 or namespace is not None:
                raise ValueError("invalid or nested tool namespace")
            found.extend(_tools(item.get("tools"), name))
        elif kind in ("function", "custom"):
            if not isinstance(name, str) or not name or len(name) > 128:
                raise ValueError("tool name missing")
            found.append({"name": name, "namespace": namespace,
                          "type": kind, "parameters": item.get("parameters")})
        # Other advertised tool types are recorded separately by their type.
        else:
            found.append({"name": (name if isinstance(name, str) else str(kind))[:128],
                          "namespace": namespace, "type": kind, "parameters": None})
    return found


def _check_arguments(arguments, schema):
    if not isinstance(schema, dict) or schema.get("type") != "object":
        raise ValueError("exec_command must advertise an object parameter schema")
    properties = schema.get("properties", {})
    required = schema.get("required", [])
    if not isinstance(properties, dict) or not isinstance(required, list):
        raise ValueError("invalid exec_command parameter schema")
    if any(key not in arguments for key in required):
        raise ValueError("fixed arguments omit an advertised required parameter")
    if schema.get("additionalProperties") is False and any(key not in properties for key in arguments):
        raise ValueError("fixed arguments contain an unadvertised parameter")
    types = {"string": str, "boolean": bool, "object": dict, "array": list,
             "integer": int, "number": (int, float), "null": type(None)}
    for key, value in arguments.items():
        definition = properties.get(key, {})
        kind = definition.get("type") if isinstance(definition, dict) else None
        if isinstance(kind, str) and kind in types:
            if not isinstance(value, types[kind]) or kind in ("integer", "number") and isinstance(value, bool):
                raise ValueError("fixed argument does not match advertised primitive type")


def _sse(index, item):
    response_id = f"fixture-response-{index}"
    events = [
        {"type": "response.created",
         "response": {"id": response_id, "status": "in_progress", "output": []}},
        {"type": "response.output_item.done", "output_index": 0, "item": item},
        {"type": "response.completed", "response": {
            "id": response_id, "status": "completed", "output": [item],
            "usage": {"input_tokens": 0, "input_tokens_details": None,
                      "output_tokens": 0, "output_tokens_details": None, "total_tokens": 0},
        }},
    ]
    return "".join(
        f"event: {event['type']}\ndata: {json.dumps(event, separators=(',', ':'))}\n\n"
        for event in events
    ).encode()


class Server:
    """Context-managed HTTP fixture with a fixed sequence and safe evidence."""

    def __init__(self, steps, output_dir, *, timeout=90, max_body=MAX_BODY):
        if not isinstance(steps, list) or not steps or len(steps) > 32:
            raise ValueError("provide between 1 and 32 fixed response steps")
        if not isinstance(timeout, (int, float)) or not 0 < timeout <= 300:
            raise ValueError("timeout must be positive and at most 300 seconds")
        if type(max_body) is not int or not 0 < max_body <= MAX_BODY:
            raise ValueError("max_body must be positive and at most 4 MiB")
        self.steps = deepcopy(steps)
        for index, step in enumerate(self.steps):
            if not isinstance(step, dict):
                raise ValueError("step must be an object")
            if step.get("kind") == "tool":
                if step.get("name") != "exec_command" or not isinstance(step.get("arguments"), dict):
                    raise ValueError("only fixed exec_command object arguments are accepted")
                if index == len(steps) - 1:
                    raise ValueError("the last step must be final")
                expected = step.get("expect_output_contains", [])
                if isinstance(expected, str):
                    expected = [expected]
                if (not isinstance(expected, list) or len(expected) > 32
                        or any(not isinstance(value, str) or not value or len(value) > 4096 for value in expected)):
                    raise ValueError("expect_output_contains must contain bounded non-empty strings")
                step["expect_output_contains"] = expected
            elif step.get("kind") == "final":
                if not isinstance(step.get("text"), str) or len(step["text"]) > 65536:
                    raise ValueError("final text must be a bounded string")
            else:
                raise ValueError("unknown fixture step kind")
        json.dumps(self.steps, allow_nan=False)
        self.output_dir = Path(output_dir)
        if self.output_dir.is_symlink():
            raise ValueError("output_dir must not be a symlink")
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.receipt_path = self.output_dir / "responses-summary.json"
        if self.receipt_path.exists() or self.receipt_path.is_symlink():
            raise ValueError("fixture evidence already exists")
        self.timeout, self.max_body = timeout, max_body
        self.index, self.request_count = 0, 0
        self.records, self.errors = [], []
        self.pending_call = None
        self.pending_expectations = []
        self.lock = threading.Lock()
        self.httpd = self.worker = self.timer = None
        self.started = None
        self.port = None
        self.base_url = None

    def summary(self):
        with self.lock:
            return {
                "schema": 1, "scope": "fixed loopback Responses fixture; no real model",
                "bind_host": "127.0.0.1", "port": self.port, "no_external_forwarding": True,
                "expected_steps": len(self.steps), "completed_steps": self.index,
                "request_count": self.request_count, "requests": deepcopy(self.records),
                "errors": list(self.errors), "complete": self.index == len(self.steps) and not self.errors,
                "elapsed_seconds": round(time.monotonic() - self.started, 3) if self.started else 0,
            }

    def _save(self):
        pending = self.receipt_path.with_suffix(".json.tmp")
        pending.write_text(json.dumps(self.summary(), indent=2, sort_keys=True) + "\n")
        pending.replace(self.receipt_path)

    def assert_complete(self):
        result = self.summary()
        if not result["complete"] or self.pending_call is not None:
            raise AssertionError("Responses fixture did not complete its fixed protocol sequence")
        return result

    @property
    def assertions(self):
        result = self.summary()
        return {
            "all_fixed_steps_served": result["completed_steps"] == result["expected_steps"],
            "no_protocol_errors": not result["errors"],
            "exact_request_count": result["request_count"] == result["expected_steps"],
            "all_tool_outputs_returned": self.pending_call is None,
        }

    def __enter__(self):
        if self.httpd is not None:
            raise RuntimeError("fixture cannot be entered twice")
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_args):
                pass  # Never log URL/header/body values from a request.

            def setup(self):
                super().setup()
                self.connection.settimeout(5)

            def _error(self, status, reason):
                with owner.lock:
                    if len(owner.errors) < 40:
                        owner.errors.append(reason)
                payload = json.dumps({"error": "fixed Responses fixture rejected request"}).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(payload)
                self.close_connection = True

            def _count_request(self):
                with owner.lock:
                    owner.request_count += 1
                    count = owner.request_count
                if count > len(owner.steps) + 2:
                    # shutdown must run outside the request handling thread.
                    threading.Thread(target=owner.httpd.shutdown, daemon=True).start()
                    self._error(429, "request count exceeded fixture bound")
                    return None
                return count

            def _reject_method(self):
                if self._count_request() is not None:
                    self._error(405, "unexpected HTTP method")

            do_GET = do_HEAD = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = do_CONNECT = _reject_method

            def do_POST(self):
                count = self._count_request()
                if count is None:
                    return
                if self.path != "/v1/responses":
                    return self._error(404, "unexpected HTTP path")
                if any(self.headers.get(name) for name in ("Authorization", "Proxy-Authorization", "Cookie")):
                    return self._error(400, "credential-bearing request was rejected without recording headers")
                if self.headers.get("Transfer-Encoding") or self.headers.get("Content-Encoding", "identity") != "identity":
                    return self._error(415, "request must use uncompressed Content-Length JSON")
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                except ValueError:
                    return self._error(400, "invalid Content-Length")
                if not 0 < length <= owner.max_body:
                    return self._error(413, "request body outside fixture size bound")
                try:
                    encoded = self.rfile.read(length)
                    if len(encoded) != length:
                        raise ValueError("incomplete request body")
                    body = json.loads(encoded)
                    if not isinstance(body, dict) or body.get("stream") is not True:
                        raise ValueError("request must be an object with stream=true")
                    with owner.lock:
                        if owner.errors or owner.index >= len(owner.steps):
                            raise ValueError("no pending fixed response step")
                        inventory = _tools(body.get("tools", []))
                        inputs = body.get("input", [])
                        if not isinstance(inputs, list):
                            raise ValueError("input must be an array")
                        record = {
                            "request": count,
                            "tool_names": [
                                f"{tool['namespace']}.{tool['name']}" if tool["namespace"] else tool["name"]
                                for tool in inventory
                            ],
                            "input_item_count": len(inputs),
                        }
                        if owner.pending_call is not None:
                            outputs = [item for item in inputs if isinstance(item, dict)
                                       and item.get("type") == "function_call_output"
                                       and item.get("call_id") == owner.pending_call]
                            if len(outputs) != 1 or "output" not in outputs[0]:
                                raise ValueError("matching previous function_call_output missing or duplicated")
                            output = json.dumps(outputs[0]["output"], sort_keys=True, separators=(",", ":")).encode()
                            record["returned_call_id"] = owner.pending_call
                            record["tool_output_sha256"] = hashlib.sha256(output).hexdigest()
                            record["tool_output_bytes"] = len(output)
                            text = outputs[0]["output"] if isinstance(outputs[0]["output"], str) else output.decode()
                            matched = all(expected in text for expected in owner.pending_expectations)
                            record["output_expectations_checked"] = len(owner.pending_expectations)
                            record["output_expectations_passed"] = matched
                            if not matched:
                                owner.records.append(record)
                                raise ValueError("tool output did not contain all fixed expected markers")
                            owner.pending_call = None
                            owner.pending_expectations = []
                        step = owner.steps[owner.index]
                        index = owner.index + 1
                        if step["kind"] == "tool":
                            matches = [tool for tool in inventory if tool["type"] == "function"
                                       and tool["name"] == step["name"]]
                            if len(matches) != 1:
                                raise ValueError("exact exec_command function not uniquely advertised")
                            tool = matches[0]
                            _check_arguments(step["arguments"], tool["parameters"])
                            call_id = f"fixture-{index}"
                            item = {
                                "type": "function_call", "id": f"fixture-item-{index}",
                                "call_id": call_id, "name": tool["name"], "status": "completed",
                                "arguments": json.dumps(step["arguments"], separators=(",", ":")),
                            }
                            if tool["namespace"]:
                                item["namespace"] = tool["namespace"]
                            owner.pending_call = call_id
                            owner.pending_expectations = step["expect_output_contains"]
                            record["sent_call_id"] = call_id
                            record["selected_tool"] = tool["name"]
                            record["selected_namespace"] = tool["namespace"]
                        else:
                            item = {
                                "type": "message", "id": f"fixture-item-{index}",
                                "role": "assistant", "status": "completed",
                                "content": [{"type": "output_text", "text": step["text"], "annotations": []}],
                            }
                        payload = _sse(index, item)
                        owner.records.append(record)
                        owner.index += 1
                except (ValueError, TypeError, KeyError, OSError) as error:
                    # Error messages are locally authored; do not print malformed JSON.
                    reason = str(error) if type(error) is ValueError and not isinstance(error, json.JSONDecodeError) else type(error).__name__
                    return self._error(400, reason)
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(payload)))
                self.send_header("Connection", "close")
                self.end_headers()
                try:
                    self.wfile.write(payload)
                except OSError:
                    with owner.lock:
                        owner.errors.append("response connection closed before SSE delivery")
                self.close_connection = True

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.httpd.daemon_threads = True
        self.port = self.httpd.server_port
        self.base_url = f"http://127.0.0.1:{self.port}/v1"
        self.started = time.monotonic()
        self.worker = threading.Thread(target=self.httpd.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True)
        self.worker.start()

        def expired():
            with self.lock:
                self.errors.append("fixture lifetime exceeded deadline")
            self.httpd.shutdown()

        self.timer = threading.Timer(self.timeout, expired)
        self.timer.daemon = True
        self.timer.start()
        return self

    def __exit__(self, *_exc):
        self.timer.cancel()
        self.httpd.shutdown()
        self.httpd.server_close()
        self.worker.join(timeout=5)
        if self.worker.is_alive():
            with self.lock:
                self.errors.append("fixture listener did not stop")
        self._save()
        return False
