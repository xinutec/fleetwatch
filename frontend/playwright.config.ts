import { defineConfig, devices } from '@playwright/test';
import { phoneConfig } from '@xinutec/ui-harness/config';
import harness from './e2e/harness.mjs';

/**
 * Phone-width layout harness. Runs against the real production build, served by
 * the shared harness. `npm run ui-check`.
 *
 * Everything shared — the Pixel geometry, the port, the static server — comes
 * from @xinutec/ui-harness, including the rule this file used to have to state
 * for itself: the viewport lives in the PROJECT `use`, not the global one,
 * because a device spread carries its own viewport and project-level `use`
 * overrides global. That is the mistake that once ran life's "phone" tests at
 * 1280×720. What this app says about itself is in e2e/harness.mjs.
 */
export default defineConfig(phoneConfig(harness, devices));
