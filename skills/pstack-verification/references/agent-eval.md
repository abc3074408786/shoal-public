# Compare agent behavior

This is a behavioral comparison of a skill, prompt, or instruction change. It
does not itself establish product acceptance or replace the application's tests.

## Define the comparison before running it

Name the decision, baseline, one changed condition, representative cases, trial
budget, and success criteria. Choose objective checks where behavior is
executable; use three to six concrete rubric criteria for genuinely qualitative
judgment. Set thresholds before seeing outputs. Keep held-out cases separate
from examples used to tune the change.

Freeze the relevant checkout, skill/prompt bytes, fixtures, tool availability,
and model settings. Use comparable environments, inputs, and budgets. Record any
unavoidable difference instead of attributing it to the changed instruction.

## Isolate and blind

Give each run its own working directory and owned state. Separate runs from each
other and from the user's active checkout. Candidate-visible paths, prompts,
and metadata must not reveal the condition, rubric, alternative candidates,
model identities, or that it is being compared. Retain genuine task assets such
as ordinary test files; remove experimental labels rather than falsifying the
organic project. Keep the label/key mapping and rubric outside candidate access.

Use the same natural user request for paired runs. Do not ask candidates to name
the skill, principle, or experimental chain they followed. Evaluate behavior and
artifacts instead of compliance claims. Only the selected variant's naturally
available resources belong in that candidate environment.

## Run with the available host

Use only task-authorized agent execution and the host's real APIs. Repository
rules determine whether candidates may edit code or only investigate. No
hardcoded Cursor agent type, model family, fast mode, or API key is required.
Do not launch candidates that would violate the repository's delegation rules.

Prefer repeated paired trials within the agreed budget; parallelism is optional
and must respect isolation and concurrency limits. If comparing models, record
that model identity also changed. A judge receives sanitized output labels and
the rubric, not the label/key mapping. Score both conditions together on the
same scale. Use an independent judge identity, preferably a different model
family when actually available; report missing independence rather than invent it.

If the host cannot launch isolated candidates or an independent judge, deliver
the runnable protocol, fixtures, and missing prerequisite as **not run**. Do not
replace missing agent trials with hand-written answers and call them an eval.

## Verify and interpret

Run the objective checks against each candidate's actual artifacts. Collect only
the native traces explicitly belonging to those runs, where the host exposes
them and the task permits access. Do not search unrelated conversation folders
or access private reasoning. If file-access/tool events are unavailable, mark
chain verification unavailable and restrict claims to observable results.

Read the candidate outputs and compare them with checks and blinded judgment.
An unreadable artifact, missing tool, timeout, or failed check is a reported
outcome, not a dropped sample. Resolve grader disagreements with evidence or
report uncertainty; do not rerun until the preferred condition wins.

Report cases/trials, frozen inputs and settings, score definition, per-case
results, failures, judge independence/blinding, trace availability, and limits.
Separate functional success, instruction following, and measured time/token
cost. Unknown usage stays unknown. A small comparison supports only a scoped
recommendation; preserve outputs, grading inputs, and reproducible commands.

Promotion is a recommendation until the authorized owner adopts the change.
Do not edit the global skill, merge, or deploy as part of a comparison alone.
