---
name: wagner
description: Multi-role Autonomous Software Factory methodology orchestrating PO, Business Expert, Architect, Developer, QA (Test Factory), and OPS (CI/CD, RUN & Rollback) agents.
---

# Wagner Autonomous Software Factory Skill

This workspace implements the **Wagner Software Factory**, orchestrated by a Rust Axum backend calling Antigravity (`agy`) subprocesses and visualized in a Vue 3 brutalist Kanban IDE.

## Core Principles

1. **6 Multi-Disciplinary Roles from Design Onward**:
   - **PO (`Product Owner`)**: User-centric value, positive experience, agile backlog & acceptance criteria.
   - **EXPERT (`Expert Métier`)**: Ubiquitous domain language, business invariants, and edge cases.
   - **ARCHI (`Software Factory Architect`)**: Homogeneity of the Software Factory, modular compatibility, rollback-safe boundaries.
   - **DEV (`Software Crafts(wo)man`)**: Clean code, TDD/BDD, SCM conflict prevention, non-destructive changes.
   - **QA (`Connected Test Factory`)**: Automated integration and Gherkin BDD business test scenarios serving as both a regression safety net and living documentation.
   - **OPS (`DevSecFinOps & SRE`)**: Automated Build & Release CI/CD pipelines, auto-generated Release Notes, RUN observability, and first-class Rollback scripts from day one.

2. **6 Modular Factory Bricks**:
   - Only enable the bricks needed for a given project (`project_management`, `scm_security`, `test_factory`, `living_documentation`, `continuous_deployment`, `run_and_rollback`).
