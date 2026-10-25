### Scenario: text input

Judge whether text was entered into the target field.

**Evidence:** **[Screen before action]** vs **[Screen after action]** — field
content and focus.

### Step 1 — did the text land where `goal` asked?

Read the field `goal` names in both frames. Only a **difference between the two**
counts as entered text — characters already present in the before frame were not
typed by this action. `clear_first` means the before content should be gone.

**Which field got the text is part of `goal`.** The expected text in the
neighbouring field did not achieve `goal`.

- **Yes, the text `goal` describes, in the field `goal` says** → `pass`. Done.
- **No** → go to step 2 and find out why.

### Step 2 — why not?

Read the pointer in the before frame — it sits where the input was aimed.

- **The pointer is on the neighbouring field, or the text went there** →
  `fail`, `precision_miss`. The aim was off by one field.
- **The pointer is on an unrelated element, or the text went elsewhere** →
  `fail`, `wrong_operation`.
- **Nothing changed anywhere** → `wrong_operation`.

If `auto_enter`: form may have submitted — a navigation, results, or dialog close
that is a **difference between the two frames** counts as achieving `goal`.

**`loading_detected`:** post-submit spinner or page load in progress.

### Reasoning (hard rule)

Max **3 sentences**, plain prose — no numbered lists or section headers.

1. Field content in before, then in after — what differs.
2. Pass or fail + `failure_cause` when fail.
3. Optional: loading after auto-submit.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
