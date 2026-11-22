# Velella

<!-- impeccable:product-schema 1 -->

## Platform

web

The existing React interface runs inside the Windows Electron desktop application.

## Product Purpose

Velella is a local workbench for opening project folders and repositories and using native Agent CLI terminals inside their workspaces.

## Operating Context

The current user works with local Windows projects. Existing UI copy is Simplified Chinese; project names and Windows paths are real navigation content.

## Capabilities and Constraints

- Existing project navigation, opening a folder, and creating a project already have application callbacks.
- Preserve project data, workspace layouts, terminal history, saved CLI settings and active processes when changing the homepage.
- Do not invent project activity, recent-use dates, running-task counts or repository status that the homepage has not received.

## Brand Commitments

Keep the existing Velella name, three-fish asset and surrounding desktop workbench identity. The user accepts approximate Orca-like everyday workflows rather than exhaustive feature parity.

## Evidence on Hand

- `desktop/src/ProjectHome.tsx` and `WorkspaceApp.tsx` own the existing homepage and its real callbacks.
- `desktop/src/theme.css`, `project-shell.css` and `DESIGN.md` describe the existing interface.
- On 2026-11-02 the user confirmed that existing projects and new projects are equally important on the homepage.

## Product Principles

- Opening an existing project and starting a new project should both be straightforward.
- Prefer direct entry into work and minimize unnecessary confirmation steps.
- Keep current project and terminal state intact across interface changes.
