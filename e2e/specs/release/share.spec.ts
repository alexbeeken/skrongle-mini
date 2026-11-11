// Link sharing on a release server (#40, #16): the owner of a course makes a
// share link in the Share dialog, a learner opens it and has the course, and
// removing the link takes it from their list.
import { createCourse, makeShareLink, shareRow } from "../../lib/app.ts";
import { expect, newLearner, newPerson, ownerPerson, test } from "../../lib/release.ts";

test("a share link gives a learner the course until the owner removes it", async ({ browser }) => {
  const { page: ownerPage } = await ownerPerson(browser);
  const id = await createCourse(ownerPage);
  await ownerPage.getByTestId("generate-close").click();
  await ownerPage.getByTestId("course-edit-share").click();
  const link = await makeShareLink(ownerPage, "A friend");
  await expect(shareRow(ownerPage, "A friend")).toHaveAttribute("data-opened", "false");

  const { page } = await newLearner(browser, "shared");
  await page.goto(link);
  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toHaveCount(1);
  await expect(card.getByTestId("courses-edit")).toHaveCount(0);
  expect((await page.goto(`/courses/${id}`))?.status()).toBe(404); // read only: no editor
  await page.goto("/lessons");
  await expect(page).toHaveURL(/\/lessons$/);

  // The owner sees it opened, then removes the link.
  await ownerPage.goto(`/courses/${id}?share=1`);
  await expect(shareRow(ownerPage, "A friend")).toHaveAttribute("data-opened", "true");
  await shareRow(ownerPage, "A friend").getByTestId("course-share-remove").click();
  await expect(ownerPage.getByTestId("course-share-notice")).toContainText("Link removed");
  await expect(ownerPage.getByTestId("course-share-row")).toHaveCount(0);

  // The course leaves the learner's list, and the link no longer works for anyone else.
  await page.goto("/courses");
  await expect(card).toHaveCount(0);
  const other = await newPerson(browser);
  await other.page.goto(link);
  await expect(other.page).not.toHaveURL(new RegExp(`/courses/${id}`));
});
