# Git Worktree Pilot Guide

## Purpose

Use this protocol with 5–10 developers who routinely work across multiple branches or Git worktrees. Measure whether Git Manager makes it easier to create, inspect, reopen, and safely clean up local workspaces. No pilot sessions have been run yet.

## Preflight

- Use a release candidate built from the exact commit being evaluated. The public v0.1.10 archives predate the `main` snapshot as of 2026-09-30; do not use them to evaluate the newer worktree workflow.
- Recruit participants who already use Git worktrees or regularly switch between parallel branches. Do not contact anyone until the repository owner authorizes the outreach and provides an audience.
- Ask participants to use a disposable repository or a copy with no valuable uncommitted work. Do not ask them to force-remove a worktree from an active project.
- Record consent, app version, OS/version, and task outcomes. Do not collect repository contents, credentials, or private remote URLs.
- Test on physical Windows and macOS devices as well as Linux where available. CI results alone are not physical-device validation.

## Session Plan

Plan for a 20-minute moderated session. Give the participant the task prompts without first explaining where each control is.

1. Open or clone the sample repository and find its worktrees.
2. Create a worktree from a new branch for a parallel change.
3. Make one tracked edit and add one untracked file in the sample worktree. Find both in the status summary.
4. Open that worktree inside Git Manager and identify its branch, upstream/merge state, and last-opened-in-app label.
5. Start the normal cleanup preview for a disposable worktree, explain what the preview says will happen, then cancel.
6. If the participant is comfortable and the fixture is disposable, remove a clean, merged worktree through the normal confirmation flow.

If any state is unexpected, stop before deletion and preserve the fixture for diagnosis. Do not use force removal as a required task.

## Record Per Task

- Completed without help: yes/no.
- Time to completion.
- Misclicks, backtracking, or misunderstood labels.
- Any unexpected Git state or data-loss concern.
- Confidence in understanding the cleanup preview, rated 1–5.
- Participant's exact words about what was unclear or reassuring.

## Exit Criteria

Treat these as proposed gates, not results:

- At least 80% of participants finish the create, inspect, reopen, and preview tasks without facilitator help (4 of 5 for a five-person pilot).
- No participant loses work or removes the wrong worktree.
- Median cleanup-preview confidence is at least 4/5.
- Any incorrect status, unsafe cleanup, or unclear destructive confirmation blocks wider distribution until fixed and retested.

## Follow-up Questions

1. What did you expect the Worktrees view to show that it did not?
2. Could you tell which worktree was dirty, unpushed, merged, or locked?
3. Did the last-opened-in-app label help? Did you understand that it does not track terminal or editor activity?
4. Before confirming cleanup, could you tell which directory and Git metadata would be affected?
5. What task would make you return to Git Manager instead of using Git directly?
6. What would prevent you from using this with your next parallel branch?

## Recruitment Message

> I am looking for 5–10 developers who regularly work on multiple Git branches or worktrees to try a short desktop-app pilot. The session takes about 20 minutes and uses a disposable repository. I will observe the workflow and collect task outcomes and usability feedback; I will not collect repository contents or credentials. The current build is an early pilot candidate, and this is not a request to install it on an active project.

## Short Demo Outline

1. Open a sample repository and show the Worktrees-first view.
2. Create and open a parallel worktree.
3. Point out local/upstream state and the limited, app-local last-opened signal.
4. Open a cleanup preview, read the impact summary, and cancel.

This is a recording outline only; no demo video or worktree-focused screenshot has been produced in this task.
