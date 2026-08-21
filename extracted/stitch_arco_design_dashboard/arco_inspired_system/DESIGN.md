---
name: Arco-Inspired System
colors:
  surface: "#f8f9ff"
  surface-dim: "#cfdbee"
  surface-bright: "#f8f9ff"
  surface-container-lowest: "#ffffff"
  surface-container-low: "#eff4ff"
  surface-container: "#e5eeff"
  surface-container-high: "#dee9fc"
  surface-container-highest: "#d8e3f7"
  on-surface: "#111c2a"
  on-surface-variant: "#434656"
  inverse-surface: "#263140"
  inverse-on-surface: "#eaf1ff"
  outline: "#737688"
  outline-variant: "#c3c5d9"
  surface-tint: "#004fe5"
  primary: "#0047cf"
  on-primary: "#ffffff"
  primary-container: "#165dff"
  on-primary-container: "#eeefff"
  inverse-primary: "#b6c4ff"
  secondary: "#5c5f60"
  on-secondary: "#ffffff"
  secondary-container: "#dee0e2"
  on-secondary-container: "#606365"
  tertiary: "#9a2f00"
  on-tertiary: "#ffffff"
  tertiary-container: "#c43e00"
  on-tertiary-container: "#ffece6"
  error: "#ba1a1a"
  on-error: "#ffffff"
  error-container: "#ffdad6"
  on-error-container: "#93000a"
  primary-fixed: "#dce1ff"
  primary-fixed-dim: "#b6c4ff"
  on-primary-fixed: "#00164f"
  on-primary-fixed-variant: "#003bb0"
  secondary-fixed: "#e1e2e4"
  secondary-fixed-dim: "#c5c6c8"
  on-secondary-fixed: "#191c1e"
  on-secondary-fixed-variant: "#444749"
  tertiary-fixed: "#ffdbd0"
  tertiary-fixed-dim: "#ffb59d"
  on-tertiary-fixed: "#390c00"
  on-tertiary-fixed-variant: "#832700"
  background: "#f8f9ff"
  on-background: "#111c2a"
  surface-variant: "#d8e3f7"
  success: "#00B42A"
  warning: "#FF7D00"
  danger: "#F53F3F"
  link: "#165DFF"
  border: "#E5E6EB"
  fill-hover: "#F2F3F5"
  text-primary: "#1D2129"
  text-secondary: "#4E5969"
  text-tertiary: "#86909C"
  bg-white: "#FFFFFF"
typography:
  h1:
    fontFamily: Inter
    fontSize: 36px
    fontWeight: "600"
    lineHeight: 44px
  h2:
    fontFamily: Inter
    fontSize: 30px
    fontWeight: "600"
    lineHeight: 38px
  h3:
    fontFamily: Inter
    fontSize: 24px
    fontWeight: "600"
    lineHeight: 32px
  h4:
    fontFamily: Inter
    fontSize: 20px
    fontWeight: "600"
    lineHeight: 28px
  body-lg:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: "400"
    lineHeight: 24px
  body-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: "400"
    lineHeight: 22px
  body-sm:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: "400"
    lineHeight: 20px
  label-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: "500"
    lineHeight: 22px
  label-sm:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: "500"
    lineHeight: 20px
  h1-mobile:
    fontFamily: Inter
    fontSize: 28px
    fontWeight: "600"
    lineHeight: 36px
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 32px
  2xl: 48px
  gutter: 16px
  container-max: 1200px
---

> 历史设计输入：本文档记录最初的 Arco 风格视觉方向，不是当前实现规范。实际颜色、间距、组件、响应式和无障碍要求以 [`docs/ui-design-system.md`](../../../docs/ui-design-system.md) 与 `frontend/src/styles/_tokens.scss` 为准。

## Brand & Style

This design system is built on the principles of **Modern Enterprise Professionalism**. It prioritizes clarity, efficiency, and scalability, making it ideal for complex SaaS platforms, internal tools, and data-heavy dashboards.

The visual style is **Corporate Modern**, characterized by a balanced use of white space, a structured information hierarchy, and a refined interface that stays out of the user's way. The emotional response is one of reliability and precision. Key attributes include:

- **Clarity:** Unambiguous visual cues and high legibility.
- **Efficiency:** Compact but breathable layouts that facilitate rapid data processing.
- **Consistency:** A rigorous logic applied to spacing, rounding, and color usage to reduce cognitive load.

## Colors

The palette is centered around a signature **Arco Blue** (#165DFF), representing intelligence and stability.

- **Primary:** Used for main actions, active states, and focus indicators.
- **Neutrals:** A multi-step gray scale is used to define information hierarchy. Text is rarely pure black, instead using `#1D2129` for primary content to reduce eye strain.
- **Functional Colors:** Success, Warning, and Danger colors follow standard enterprise conventions to ensure immediate recognition of system status.
- **Backgrounds:** Use subtle variations of gray (e.g., `#F7F8FA`) to differentiate between the canvas and container surfaces.

## Typography

The system utilizes **Inter** for its exceptional legibility in digital interfaces and neutral character.

- **Hierarchy:** Maintain a clear distinction between headings and body text using weight (Semi-Bold for headers) and size.
- **Default Size:** The standard body size is **14px** (`body-md`), which provides an optimal balance of density and readability for enterprise applications.
- **Alignment:** Text should generally be left-aligned to support natural reading patterns.
- **Scaling:** For mobile views, large headings scale down to prevent excessive wrapping while maintaining relative importance.

## Layout & Spacing

This design system uses an **8px grid system** to ensure mathematical harmony across all components and layouts.

- **Grid Model:** A 12-column fluid grid for desktop with 16px gutters. For sidebars and navigation panels, fixed widths (e.g., 200px or 240px) are preferred to ensure consistent navigation.
- **Breakpoints:**
  - Mobile: < 576px (4 columns, 16px margins)
  - Tablet: 576px - 992px (8 columns, 24px margins)
  - Desktop: > 992px (12 columns, 32px margins)
- **Density:** Provide "Comfortable" (16px padding) and "Compact" (8px padding) variants for data tables and lists depending on the user's information density needs.

## Elevation & Depth

Hierarchy is conveyed through **Tonal Layering** and **Subtle Shadows**.

- **Layers:** Use different background shades to stack elements. Level 0 is the main background, Level 1 is a card or container, and Level 2 is an overlay or popover.
- **Shadows:** Avoid heavy, dark shadows. Use a "Soft Diffused" style with low opacity (approx. 8-12%) and a slight vertical offset to suggest elevation without creating visual clutter.
- **Borders:** For flat surfaces (like table cells or input fields), use a subtle 1px border (`#E5E6EB`) rather than shadows to define boundaries.

## Shapes

The system uses a **Soft** geometry to feel approachable yet professional.

- **Base Radius:** 4px (`0.25rem`) is the standard for buttons, inputs, and small components.
- **Container Radius:** Cards and modals should use 8px (`0.5rem`) to create a clear container-child relationship with the smaller elements inside them.
- **Circle:** Used exclusively for avatars and specific status icons.
- **Consistency:** Never mix sharp corners with rounded corners within the same component group.

## Components

- **Buttons:**
  - Primary: Solid Blue background with white text.
  - Secondary: Ghost style with blue border and text, or light gray background.
  - Height: Standardized at 32px (Small), 36px (Medium/Default), and 40px (Large).
- **Input Fields:** 1px border (`#E5E6EB`) that transitions to the primary blue on focus. Use a 4px corner radius.
- **Cards:** White background, 8px corner radius, and either a 1px border or a very soft shadow (Level 1 elevation).
- **Chips/Tags:** Used for categorization. These should have a light tinted background (e.g., Light Blue fill for a Blue tag) and 2px rounded corners.
- **Lists:** High-density items with 12px or 16px vertical padding. Use thin dividers to separate items without breaking the visual flow.
- **Tables:** The workhorse of enterprise UI. Use sticky headers, zebra striping (optional), and clear sorting indicators. Text within tables should stay at 14px or 12px.
