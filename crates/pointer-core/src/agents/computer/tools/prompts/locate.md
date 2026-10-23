### locate_element

Turns a description into coordinates. **It does not click, type, or change the
desktop** — it only answers where the target is on the current screenshot.

**Input**

```json
{"goal": "保存当前文档",
 "elements": [
   {"description": "搜索框"},
   {"description": "搜索按钮", "hint_box": [800, 35, 840, 55]}
]}
```
- **`goal` is required and sits at the top level** (not inside `elements[]`). One
  sentence saying what this call is for; it is shown as the card title. It does
  **not** take part in grounding — put every distinguishing detail into
  `description`.
- One `elements` array, each entry a `description`, optionally a `hint_box`.
- **Every entry must already exist on the current screen.** Do not pre-locate an
  element that only appears after clicking something — the screen has not
  changed yet, so it is not in the image and the call will fail. When order
  matters, split it: locate and click the first, wait for the UI to change, then
  locate the second.
- **Three entries maximum.** More than three is an argument error; split the call.
- The `target_desc` in the output is the `description` you passed, echoed back so
  you can match results to inputs.
- `hint_box` is four **logical pixel** values `[x1, y1, x2, y2]` — your rough
  guess at where the target is, **not** the final click coordinate. Pass it only
  when you can actually see roughly where the target sits.

**What `description` should contain**

Four parts. Missing one costs an extra grounding round.

| Part | What to write | Example |
| --- | --- | --- |
| **Appearance** | Shape, colour, the text or symbol inside it | `blue rounded button, white text 「保存」` |
| **Control type** | button / icon / menu item / text label / input field / folder | `button` |
| **Screen position** | Which region of the page (top nav bar / second tab row / right side rail / lower banner) | `to the right of the search box` |
| **Anchor** | An adjacent element that is true **only for this target** | `next to the 「撤销」 button` |

One sentence:

```
工具栏右侧的蓝色圆角按钮，白色文字「保存」，紧邻「撤销」按钮
```

**Why all four**:

- Name only (`保存`) → picks the wrong one when the label appears twice
- Name + position (`工具栏右侧的「保存」`) → usually enough
- Plus anchor (`紧邻「撤销」`) → separates two same-named elements sitting side by side

**The anchor must hold only for the target.** `搜索框右侧` works (only the search
box is to its left). `页面上方` does not (everything is up there).

**Use region names, not coordinates.** The grounder computes coordinates itself.

**Output**

```json
{"results": [
  {"status": "located",
   "target_desc": "地址栏右侧的刷新按钮",
   "summary": "框内是：圆形箭头刷新图标",
   "bbox": [800, 35, 840, 55], "confidence": "high"},
  {"status": "not_found",
   "target_desc": "『文件』菜单里的『另存为』",
   "summary": "先滚动、切换页面或等待加载，再重新定位；不要用猜测的坐标点击。画面里看到：当前截图中未能可靠确认目标"}
]}
```

Results come back **in input order**; one failure does not affect the others.
`target_desc` is the description you sent, echoed back.

**`status` alone tells you what happened**:

| `status` | Meaning | Next step |
| --- | --- | --- |
| `located` | Found; `bbox` is the target box | Pass `bbox` with the same `target_desc` to `mouse_click_at` etc. |
| `not_found` | Not on the current screen | Scroll / switch page / wait for load, then locate again |
| `not_verified` | A candidate box failed verification | Use a more distinguishing description (add an anchor, add the text) |
| `unclear` | Image too blurry or occluded | Let the UI settle first (wait for load, dismiss dialogs) |

`summary` is a sentence for you to read: on success it says what is inside the
box, on failure it says what to do next plus what was actually visible.

**`bbox` is the only position you need**

- `bbox` is the target's box on screen `[x1, y1, x2, y2]` in logical pixels.
- **Pass it unchanged to `mouse_click_at` / `mouse_hover_at` / `mouse_down` /
  `mouse_up` / `mouse_drag_to`** — those tools re-confirm inside it before
  clicking. Do not compute the centre yourself, do not shrink the box.
- `confidence` is how sure the grounder is:
  - `high` — confirmed, use it
  - `medium` — probably right, neighbours may interfere
  - `low` — retry with a different description

**Use the `summary` sentence to cross-check**

If you asked for 「搜索框」 and it says 「圆形箭头刷新图标」, that is not it — change
the description.

**When you get `located`, act on it**

`located` with `confidence: high` is enough: the target is confirmed and the
coordinate is clickable. Locating again returns slightly different coordinates
(the vision model is stochastic) — that is jitter, not extra accuracy. **Do not
call it again unless the screen visibly changed.**

**When not to use it**

- You already have this target's `bbox` from an earlier round and the screen has
  not changed — pass that `bbox` straight to `mouse_click_at`; it re-confirms
  before clicking, so there is no need to search the whole screen again.
