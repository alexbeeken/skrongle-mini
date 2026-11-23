// Add a course from a file (#252): the file Download all files makes goes back in on
// Courses (the page puts the chosen file's text in the form's box), makes a course of
// its own with the same files, and a file that would not load makes nothing.
import { expect, test } from "@playwright/test";
import { createCourse, devLogin, devReset } from "../lib/app";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a downloaded course is added from its file, with the same files", async ({ page }) => {
  await devLogin(page, "next=/courses");
  const id = await createCourse(page);
  const download = await page.request.get(`/courses/${id}/export.json`);
  expect(download.ok()).toBeTruthy();
  const text = await download.text();

  await page.goto("/courses");
  await expect(page.getByTestId("courses-import-file")).toBeVisible(); // the script shows the file field
  await page.getByTestId("courses-import-file").setInputFiles({ name: "course.json", mimeType: "application/json", buffer: Buffer.from(text) });
  await expect(page.getByTestId("courses-import-text")).toHaveValue(text);
  await page.getByTestId("courses-import-button").click();
  await expect(page).toHaveURL(/\/courses\/[^/?]+$/);
  const newId = new URL(page.url()).pathname.split("/").pop() ?? "";
  expect(newId).not.toBe(id);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");

  const files = async (course: string) => {
    const json = JSON.parse(await (await page.request.get(`/courses/${course}/export.json`)).text());
    delete json["course.json"].id;
    return json;
  };
  expect(await files(newId)).toEqual(await files(id));

  // A file that would not load: nothing is made and the page says why.
  const broken = JSON.parse(text);
  const lesson = Object.keys(broken).find((k) => k.startsWith("lessons/"))!;
  delete broken[lesson].id;
  await page.goto("/courses");
  await page.getByTestId("courses-import-text").fill(JSON.stringify(broken));
  const before = await page.getByTestId("courses-card").count();
  await page.getByTestId("courses-import-button").click();
  await expect(page.getByTestId("courses-notice")).toContainText("The course was not added");
  expect(await page.getByTestId("courses-card").count()).toBe(before);
});
