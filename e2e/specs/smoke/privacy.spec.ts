// Privacy (#74): the AI forms say where notes go. Exact: no
// model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { createCourse, devLogin } from "../../lib/app.ts";

test("the AI forms say which provider gets the notes", async ({ page }) => {
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await createCourse(page);
  // The Generate dialog's line, then Use AI's under it once the dialog is closed.
  const disclosure = page.getByTestId("generate").getByTestId("ai-disclosure");
  await expect(disclosure).toBeVisible();
  await expect(disclosure).toHaveText(/^Your notes are sent to .+ using this server's key\.$/);
  await expect(disclosure).not.toContainText("your provider");
  await expect(page.getByTestId("generate-sensitive-note")).toContainText("sensitive personal data");
  await page.getByTestId("generate-close").click();
  await expect(page.getByTestId("course-ai-form").getByTestId("ai-disclosure")).toHaveText(/^Your notes are sent to .+ using this server's key\.$/);
  await expect(page.getByTestId("course-ai-sensitive-note")).toContainText("sensitive personal data");
});
