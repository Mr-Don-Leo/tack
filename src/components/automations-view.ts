// Automations: the rule list and the trigger → conditions → actions editor.

import * as api from "../api";
import { button, el, icon, iconButton, on, render } from "../dom";
import { PRIORITY_NAMES, WEEKDAY_SHORT, describeOffset, formatRelative } from "../format";
import { state } from "../store";
import type {
  ActionType,
  Automation,
  AutomationAction,
  Board,
  Condition,
  ConditionType,
  Id,
  Label,
  List,
  Trigger,
  TriggerType,
} from "../types";
import { emptyState, guard, toast } from "./toast";
import { dropdown, toggleSwitch } from "./ui/controls";
import { confirmDialog, openModal } from "./ui/overlays";

/** Every column and label in the app, needed to label rule parameters. */
interface RuleContext {
  boards: Board[];
  lists: List[];
  labels: Label[];
}

const TRIGGER_LABELS: Record<TriggerType, string> = {
  taskCreated: "A task is created",
  taskCompleted: "A task is completed",
  taskOverdue: "A task becomes overdue",
  dueDateReached: "A due date is reached",
  taskMoved: "A task is moved between columns",
  labelAdded: "A label is added",
  labelRemoved: "A label is removed",
  scheduled: "At a scheduled time",
};

const CONDITION_LABELS: Record<ConditionType, string> = {
  hasLabel: "Has the label",
  lacksLabel: "Does not have the label",
  priorityAtLeast: "Priority is at least",
  priorityEquals: "Priority is exactly",
  inList: "Is in the column",
  inBoard: "Is on the board",
  titleContains: "Title contains",
  isOverdue: "Is overdue",
  isCompleted: "Is completed",
  isNotCompleted: "Is not completed",
  hasDueDate: "Has a due date",
  hasNoDueDate: "Has no due date",
  dueWithinMinutes: "Is due within",
};

const ACTION_LABELS: Record<ActionType, string> = {
  moveTask: "Move the task to",
  createTask: "Create a task",
  completeTask: "Mark the task complete",
  setPriority: "Set priority to",
  addLabel: "Add the label",
  removeLabel: "Remove the label",
  setDueDate: "Set the due date to",
  setReminder: "Set a reminder for",
  notify: "Send a notification",
  duplicateTask: "Duplicate the task",
  archiveTask: "Archive the task",
};

export async function renderAutomations(container: HTMLElement, onChanged: () => void): Promise<void> {
  const automations = await guard(() => api.listAutomations());
  if (!automations) return;
  const context = await loadContext();

  const view = el("div", { class: "view-padded view-narrow" });
  view.appendChild(
    el(
      "div",
      { class: "row", style: "margin-bottom:14px" },
      el(
        "div",
        { class: "col" },
        el("h1", { text: "Automations" }),
        el("p", {
          class: "meta",
          text: "Rules run in the background, whether or not this window is open.",
        }),
      ),
      el("span", { class: "spacer" }),
      button("New automation", {
        class: "btn btn-primary",
        onClick: () => void openEditor(null, context, onChanged),
      }),
    ),
  );

  if (automations.length === 0) {
    const empty = el("div", { class: "panel" });
    view.appendChild(empty);
    render(container, view);
    emptyState(
      empty,
      "No automations yet",
      "Create a rule to move, complete, label or remind automatically — for example, complete a card whenever it lands in Done.",
    );
    return;
  }

  const panel = el("div", { class: "panel" });
  for (const automation of automations) {
    panel.appendChild(automationCard(automation, context, onChanged));
  }
  view.appendChild(panel);
  render(container, view);
}

async function loadContext(): Promise<RuleContext> {
  const boards = state.data.boards;
  const lists: List[] = [];
  for (const board of boards) {
    try {
      const view = await api.boardView(board.id);
      lists.push(...view.lists);
    } catch {
      // A board that failed to load simply contributes no columns.
    }
  }
  return { boards, lists, labels: state.data.labels };
}

function automationCard(
  automation: Automation,
  context: RuleContext,
  onChanged: () => void,
): HTMLElement {
  const scope = automation.boardId
    ? (context.boards.find((board) => board.id === automation.boardId)?.name ?? "A board")
    : "All boards";

  return el(
    "div",
    { class: "automation-card" },
    toggleSwitch(
      automation.enabled,
      (enabled) => {
        void guard(async () => {
          await api.updateAutomation(automation.id, { enabled });
          onChanged();
        });
      },
      `Enable ${automation.name}`,
    ),
    el(
      "div",
      { class: "col", style: "flex:1;min-width:0" },
      el("div", { class: "row" },
        el("strong", { class: "truncate", text: automation.name }),
        el("span", { class: "pill", text: scope }),
      ),
      el("div", { class: "automation-summary", text: summarise(automation, context) }),
      automation.lastRunAt
        ? el("div", { class: "meta", text: `Ran ${formatRelative(automation.lastRunAt)} · ${automation.runCount} time${automation.runCount === 1 ? "" : "s"}` })
        : el("div", { class: "meta", text: "Has not run yet" }),
    ),
    iconButton("note", "Edit", () => void openEditor(automation, context, onChanged)),
    iconButton("trash", "Delete", () => {
      void (async () => {
        const ok = await confirmDialog({
          title: `Delete “${automation.name}”?`,
          message: "This rule will stop running immediately.",
          confirmLabel: "Delete",
          danger: true,
        });
        if (!ok) return;
        await guard(async () => {
          await api.deleteAutomation(automation.id);
          onChanged();
        });
      })();
    }),
  );
}

// Summaries ---------------------------------------------------------------

function nameOf(items: Array<{ id: Id; name: string }>, id: Id | null, fallback: string): string {
  if (!id) return fallback;
  return items.find((item) => item.id === id)?.name ?? fallback;
}

function describeTrigger(trigger: Trigger, context: RuleContext): string {
  switch (trigger.type) {
    case "taskCreated":
      return trigger.listId
        ? `a task is created in ${nameOf(context.lists, trigger.listId, "a column")}`
        : "a task is created";
    case "taskCompleted":
      return trigger.listId
        ? `a task in ${nameOf(context.lists, trigger.listId, "a column")} is completed`
        : "a task is completed";
    case "taskOverdue":
      return "a task becomes overdue";
    case "dueDateReached":
      return "a due date is reached";
    case "taskMoved": {
      const from = trigger.fromListId ? ` from ${nameOf(context.lists, trigger.fromListId, "a column")}` : "";
      const to = trigger.toListId ? ` to ${nameOf(context.lists, trigger.toListId, "a column")}` : "";
      return `a task is moved${from}${to}`;
    }
    case "labelAdded":
      return `the label ${nameOf(context.labels, trigger.labelId, "any")} is added`;
    case "labelRemoved":
      return `the label ${nameOf(context.labels, trigger.labelId, "any")} is removed`;
    case "scheduled": {
      const time = `${String(trigger.schedule.hour).padStart(2, "0")}:${String(trigger.schedule.minute).padStart(2, "0")}`;
      const days = trigger.schedule.weekdays.map((day) => WEEKDAY_SHORT[day]).filter(Boolean);
      if (trigger.schedule.dayOfMonth) return `it is day ${trigger.schedule.dayOfMonth} at ${time}`;
      return days.length ? `it is ${days.join(", ")} at ${time}` : `it is ${time} each day`;
    }
  }
}

function describeCondition(condition: Condition, context: RuleContext): string {
  switch (condition.type) {
    case "hasLabel":
      return `has the label ${nameOf(context.labels, condition.labelId, "?")}`;
    case "lacksLabel":
      return `does not have the label ${nameOf(context.labels, condition.labelId, "?")}`;
    case "priorityAtLeast":
      return `priority is at least ${PRIORITY_NAMES[condition.priority] ?? condition.priority}`;
    case "priorityEquals":
      return `priority is ${PRIORITY_NAMES[condition.priority] ?? condition.priority}`;
    case "inList":
      return `is in ${nameOf(context.lists, condition.listId, "a column")}`;
    case "inBoard":
      return `is on ${nameOf(context.boards, condition.boardId, "a board")}`;
    case "titleContains":
      return `title contains “${condition.text}”`;
    case "dueWithinMinutes":
      return `is due within ${describeOffset(condition.minutes)}`;
    default:
      return CONDITION_LABELS[condition.type].toLowerCase();
  }
}

function describeAction(action: AutomationAction, context: RuleContext): string {
  switch (action.type) {
    case "moveTask":
      return `move it to ${nameOf(context.lists, action.listId, "a column")}`;
    case "createTask":
      return `create “${action.title}”`;
    case "setPriority":
      return `set priority to ${PRIORITY_NAMES[action.priority] ?? action.priority}`;
    case "addLabel":
      return `add the label ${nameOf(context.labels, action.labelId, "?")}`;
    case "removeLabel":
      return `remove the label ${nameOf(context.labels, action.labelId, "?")}`;
    case "setDueDate":
      return `set the due date ${describeOffset(Math.abs(action.inMinutes))} from now`;
    case "setReminder":
      return `remind in ${describeOffset(Math.abs(action.inMinutes))}`;
    case "notify":
      return `notify “${action.title}”`;
    default:
      return ACTION_LABELS[action.type].toLowerCase();
  }
}

function summarise(automation: Automation, context: RuleContext): string {
  const when = `When ${describeTrigger(automation.trigger, context)}`;
  const conditions = automation.conditions.length
    ? `, and it ${automation.conditions.map((c) => describeCondition(c, context)).join(" and ")}`
    : "";
  const actions = automation.actions.map((a) => describeAction(a, context)).join(", then ");
  return `${when}${conditions} → ${actions}.`;
}

// Editor ------------------------------------------------------------------

function defaultTrigger(type: TriggerType): Trigger {
  switch (type) {
    case "taskCreated":
      return { type, listId: null };
    case "taskCompleted":
      return { type, listId: null };
    case "taskMoved":
      return { type, fromListId: null, toListId: null };
    case "labelAdded":
    case "labelRemoved":
      return { type, labelId: null };
    case "scheduled":
      return { type, schedule: { hour: 9, minute: 0, weekdays: [], dayOfMonth: null } };
    default:
      return { type } as Trigger;
  }
}

function defaultCondition(type: ConditionType, context: RuleContext): Condition {
  switch (type) {
    case "hasLabel":
    case "lacksLabel":
      return { type, labelId: context.labels[0]?.id ?? "" };
    case "priorityAtLeast":
    case "priorityEquals":
      return { type, priority: 3 };
    case "inList":
      return { type, listId: context.lists[0]?.id ?? "" };
    case "inBoard":
      return { type, boardId: context.boards[0]?.id ?? "" };
    case "titleContains":
      return { type, text: "" };
    case "dueWithinMinutes":
      return { type, minutes: 60 };
    default:
      return { type } as Condition;
  }
}

function defaultAction(type: ActionType, context: RuleContext): AutomationAction {
  switch (type) {
    case "moveTask":
      return { type, boardId: null, listId: context.lists[0]?.id ?? "" };
    case "createTask":
      return {
        type,
        boardId: null,
        listId: null,
        title: "New task",
        description: "",
        priority: 0,
        dueInMinutes: null,
        labelIds: [],
      };
    case "setPriority":
      return { type, priority: 3 };
    case "addLabel":
    case "removeLabel":
      return { type, labelId: context.labels[0]?.id ?? "" };
    case "setDueDate":
      return { type, inMinutes: 60 };
    case "setReminder":
      return { type, inMinutes: 60 };
    case "notify":
      return { type, title: "Tack", body: "{task} needs attention" };
    default:
      return { type } as AutomationAction;
  }
}

async function openEditor(
  existing: Automation | null,
  context: RuleContext,
  onChanged: () => void,
): Promise<void> {
  const draft = {
    name: existing?.name ?? "",
    boardId: existing?.boardId ?? null,
    trigger: existing?.trigger ?? defaultTrigger("taskMoved"),
    conditions: existing ? [...existing.conditions] : [],
    actions: existing ? [...existing.actions] : [defaultAction("completeTask", context)],
    enabled: existing?.enabled ?? true,
  };

  const modal = openModal({ title: existing ? "Edit automation" : "New automation" });
  const form = el("div", { class: "col", style: "gap:14px" });
  modal.body.appendChild(form);

  /** Columns belonging to the rule's scope; global rules see all of them. */
  const scopedLists = () =>
    draft.boardId ? context.lists.filter((list) => list.boardId === draft.boardId) : context.lists;

  const listOptions = () =>
    scopedLists().map((list) => ({
      value: list.id,
      label: list.name,
      detail: context.boards.find((board) => board.id === list.boardId)?.name,
    }));

  const labelOptions = () =>
    context.labels.map((label) => ({ value: label.id, label: label.name, color: label.color }));

  function paint(): void {
    const nameInput = el("input", { class: "input", type: "text", placeholder: "Name this rule", "aria-label": "Automation name" });
    nameInput.value = draft.name;
    on(nameInput, "input", () => {
      draft.name = nameInput.value;
    });

    render(
      form,
      el("label", { class: "field" }, el("span", { class: "field-label", text: "Name" }), nameInput),
      el(
        "label",
        { class: "field" },
        el("span", { class: "field-label", text: "Applies to" }),
        dropdown({
          value: draft.boardId ?? "",
          ariaLabel: "Board scope",
          options: [
            { value: "", label: "All boards" },
            ...context.boards.map((board) => ({ value: board.id, label: board.name })),
          ],
          onChange: (boardId) => {
            draft.boardId = boardId || null;
            paint();
          },
        }),
      ),
      triggerBlock(),
      conditionsBlock(),
      actionsBlock(),
    );
  }

  function triggerBlock(): HTMLElement {
    const block = el(
      "div",
      { class: "rule-block" },
      el("div", { class: "rule-block-label", text: "When" }),
      el(
        "div",
        { class: "rule-row" },
        dropdown({
          value: draft.trigger.type,
          ariaLabel: "Trigger",
          options: (Object.keys(TRIGGER_LABELS) as TriggerType[]).map((type) => ({
            value: type,
            label: TRIGGER_LABELS[type],
          })),
          onChange: (type) => {
            draft.trigger = defaultTrigger(type);
            paint();
          },
        }),
      ),
    );

    const trigger = draft.trigger;
    const row = (...children: Node[]) => el("div", { class: "rule-row" }, ...children);

    if (trigger.type === "taskCreated" || trigger.type === "taskCompleted") {
      block.appendChild(
        row(
          el("span", { class: "meta", text: "in" }),
          dropdown({
            value: trigger.listId ?? "",
            ariaLabel: "Column",
            options: [{ value: "", label: "Any column" }, ...listOptions()],
            onChange: (listId) => {
              trigger.listId = listId || null;
            },
          }),
        ),
      );
    }

    if (trigger.type === "taskMoved") {
      block.append(
        row(
          el("span", { class: "meta", text: "from" }),
          dropdown({
            value: trigger.fromListId ?? "",
            ariaLabel: "From column",
            options: [{ value: "", label: "Any column" }, ...listOptions()],
            onChange: (listId) => {
              trigger.fromListId = listId || null;
            },
          }),
        ),
        row(
          el("span", { class: "meta", text: "to" }),
          dropdown({
            value: trigger.toListId ?? "",
            ariaLabel: "To column",
            options: [{ value: "", label: "Any column" }, ...listOptions()],
            onChange: (listId) => {
              trigger.toListId = listId || null;
            },
          }),
        ),
      );
    }

    if (trigger.type === "labelAdded" || trigger.type === "labelRemoved") {
      block.appendChild(
        row(
          dropdown({
            value: trigger.labelId ?? "",
            ariaLabel: "Label",
            options: [{ value: "", label: "Any label" }, ...labelOptions()],
            onChange: (labelId) => {
              trigger.labelId = labelId || null;
            },
          }),
        ),
      );
    }

    if (trigger.type === "scheduled") {
      const time = el("input", { class: "input time-input", type: "text", maxlength: "5", "aria-label": "Time of day" });
      time.value = `${String(trigger.schedule.hour).padStart(2, "0")}:${String(trigger.schedule.minute).padStart(2, "0")}`;
      on(time, "change", () => {
        const match = /^(\d{1,2}):(\d{2})$/.exec(time.value.trim());
        if (!match) {
          time.classList.add("input-invalid");
          return;
        }
        time.classList.remove("input-invalid");
        trigger.schedule.hour = Math.min(23, Number(match[1]));
        trigger.schedule.minute = Math.min(59, Number(match[2]));
      });

      const days = el("div", { class: "chip-row" });
      for (let index = 0; index < 7; index += 1) {
        const active = trigger.schedule.weekdays.includes(index);
        const chip = el("button", {
          type: "button",
          class: `pill ${active ? "pill-accent" : ""}`.trim(),
          text: WEEKDAY_SHORT[index],
          "aria-pressed": String(active),
        });
        on(chip, "click", () => {
          trigger.schedule.weekdays = active
            ? trigger.schedule.weekdays.filter((day) => day !== index)
            : [...trigger.schedule.weekdays, index].sort((a, b) => a - b);
          paint();
        });
        days.appendChild(chip);
      }

      block.append(
        row(el("span", { class: "meta", text: "at" }), time),
        row(el("span", { class: "meta", text: "on" }), days),
        el("div", { class: "meta", text: "Leave every day unselected to run daily." }),
      );
    }

    return block;
  }

  function conditionsBlock(): HTMLElement {
    const block = el("div", { class: "rule-block" }, el("div", { class: "rule-block-label", text: "Only if" }));

    draft.conditions.forEach((condition, index) => {
      const controls: Node[] = [
        dropdown({
          value: condition.type,
          ariaLabel: "Condition",
          options: (Object.keys(CONDITION_LABELS) as ConditionType[]).map((type) => ({
            value: type,
            label: CONDITION_LABELS[type],
          })),
          onChange: (type) => {
            draft.conditions[index] = defaultCondition(type, context);
            paint();
          },
        }),
      ];

      if (condition.type === "hasLabel" || condition.type === "lacksLabel") {
        controls.push(
          dropdown({
            value: condition.labelId,
            ariaLabel: "Label",
            options: labelOptions(),
            onChange: (labelId) => {
              condition.labelId = labelId;
            },
          }),
        );
      } else if (condition.type === "priorityAtLeast" || condition.type === "priorityEquals") {
        controls.push(
          dropdown({
            value: condition.priority,
            ariaLabel: "Priority",
            options: PRIORITY_NAMES.map((label, value) => ({ value, label })),
            onChange: (priority) => {
              condition.priority = priority;
            },
          }),
        );
      } else if (condition.type === "inList") {
        controls.push(
          dropdown({
            value: condition.listId,
            ariaLabel: "Column",
            options: listOptions(),
            onChange: (listId) => {
              condition.listId = listId;
            },
          }),
        );
      } else if (condition.type === "inBoard") {
        controls.push(
          dropdown({
            value: condition.boardId,
            ariaLabel: "Board",
            options: context.boards.map((board) => ({ value: board.id, label: board.name })),
            onChange: (boardId) => {
              condition.boardId = boardId;
            },
          }),
        );
      } else if (condition.type === "titleContains") {
        const text = el("input", { class: "input", type: "text", "aria-label": "Text" });
        text.value = condition.text;
        on(text, "input", () => {
          condition.text = text.value;
        });
        controls.push(text);
      } else if (condition.type === "dueWithinMinutes") {
        controls.push(minutesInput(condition.minutes, (minutes) => {
          condition.minutes = minutes;
        }));
      }

      controls.push(
        iconButton("close", "Remove condition", () => {
          draft.conditions.splice(index, 1);
          paint();
        }),
      );
      block.appendChild(el("div", { class: "rule-row" }, ...controls));
    });

    block.appendChild(
      button("Add condition", {
        class: "btn btn-ghost btn-sm",
        style: "margin-top:8px",
        onClick: () => {
          draft.conditions.push(defaultCondition("isNotCompleted", context));
          paint();
        },
      }),
    );
    return block;
  }

  function actionsBlock(): HTMLElement {
    const block = el("div", { class: "rule-block" }, el("div", { class: "rule-block-label", text: "Then" }));

    draft.actions.forEach((action, index) => {
      const controls: Node[] = [
        dropdown({
          value: action.type,
          ariaLabel: "Action",
          options: (Object.keys(ACTION_LABELS) as ActionType[]).map((type) => ({
            value: type,
            label: ACTION_LABELS[type],
          })),
          onChange: (type) => {
            draft.actions[index] = defaultAction(type, context);
            paint();
          },
        }),
      ];

      if (action.type === "moveTask") {
        controls.push(
          dropdown({
            value: action.listId,
            ariaLabel: "Target column",
            options: listOptions(),
            onChange: (listId) => {
              action.listId = listId;
            },
          }),
        );
      } else if (action.type === "setPriority") {
        controls.push(
          dropdown({
            value: action.priority,
            ariaLabel: "Priority",
            options: PRIORITY_NAMES.map((label, value) => ({ value, label })),
            onChange: (priority) => {
              action.priority = priority;
            },
          }),
        );
      } else if (action.type === "addLabel" || action.type === "removeLabel") {
        controls.push(
          dropdown({
            value: action.labelId,
            ariaLabel: "Label",
            options: labelOptions(),
            onChange: (labelId) => {
              action.labelId = labelId;
            },
          }),
        );
      } else if (action.type === "setDueDate" || action.type === "setReminder") {
        controls.push(minutesInput(action.inMinutes, (minutes) => {
          action.inMinutes = minutes;
        }));
      } else if (action.type === "notify") {
        const title = el("input", { class: "input", type: "text", placeholder: "Title", "aria-label": "Notification title" });
        title.value = action.title;
        on(title, "input", () => {
          action.title = title.value;
        });
        const body = el("input", { class: "input", type: "text", placeholder: "Body — {task} is replaced by the title", "aria-label": "Notification body" });
        body.value = action.body;
        on(body, "input", () => {
          action.body = body.value;
        });
        controls.push(title, body);
      } else if (action.type === "createTask") {
        const title = el("input", { class: "input", type: "text", placeholder: "Task title", "aria-label": "Task title" });
        title.value = action.title;
        on(title, "input", () => {
          action.title = title.value;
        });
        controls.push(
          title,
          dropdown({
            value: action.listId ?? "",
            ariaLabel: "Column",
            options: [{ value: "", label: "First column" }, ...listOptions()],
            onChange: (listId) => {
              action.listId = listId || null;
            },
          }),
        );
      }

      controls.push(
        iconButton("close", "Remove action", () => {
          draft.actions.splice(index, 1);
          paint();
        }),
      );
      block.appendChild(el("div", { class: "rule-row" }, ...controls));
    });

    block.appendChild(
      button("Add action", {
        class: "btn btn-ghost btn-sm",
        style: "margin-top:8px",
        onClick: () => {
          draft.actions.push(defaultAction("notify", context));
          paint();
        },
      }),
    );
    return block;
  }

  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button(existing ? "Save changes" : "Create automation", {
      class: "btn btn-primary",
      onClick: () => {
        if (!draft.name.trim()) {
          toast("Give the automation a name", "error");
          return;
        }
        if (draft.actions.length === 0) {
          toast("Add at least one action", "error");
          return;
        }
        void guard(async () => {
          if (existing) {
            await api.updateAutomation(existing.id, {
              name: draft.name,
              boardId: draft.boardId,
              trigger: draft.trigger,
              conditions: draft.conditions,
              actions: draft.actions,
            });
          } else {
            await api.createAutomation({
              name: draft.name,
              boardId: draft.boardId,
              trigger: draft.trigger,
              conditions: draft.conditions,
              actions: draft.actions,
              enabled: draft.enabled,
            });
          }
          modal.close();
          onChanged();
        });
      },
    }),
  );

  paint();
}

/** A minutes field presented in the friendliest unit the value divides into. */
function minutesInput(value: number, onChange: (minutes: number) => void): HTMLElement {
  const amount = el("input", { class: "input", type: "number", min: "0", style: "width:80px", "aria-label": "Amount" });
  let unit = value % 1440 === 0 && value !== 0 ? 1440 : value % 60 === 0 && value !== 0 ? 60 : 1;
  amount.value = String(Math.round(value / unit));

  const commit = () => {
    const parsed = Math.max(0, Number.parseInt(amount.value, 10) || 0);
    amount.value = String(parsed);
    onChange(parsed * unit);
  };
  on(amount, "change", commit);

  return el(
    "div",
    { class: "row" },
    amount,
    dropdown({
      value: unit,
      ariaLabel: "Unit",
      options: [
        { value: 1, label: "minutes" },
        { value: 60, label: "hours" },
        { value: 1440, label: "days" },
      ],
      onChange: (next) => {
        unit = next;
        commit();
      },
    }),
    icon("clock", 14),
  );
}
