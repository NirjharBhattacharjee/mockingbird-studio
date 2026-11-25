# Signing in to YouTube

mockingbird-studio posts to your channel through an OAuth client you make in
your own Google Cloud project. The app ships no client of its own, so your
uploads count against your project's quota and nobody else's.

You do this once. It takes about ten minutes.

## 1. Make a Google Cloud project

1. Go to [console.cloud.google.com](https://console.cloud.google.com/) and
   sign in with the Google account that owns the channel.
2. Make a new project. Any name works, such as `mockingbird-studio`.

## 2. Turn on the YouTube Data API

In the project, open **APIs & Services → Library**, find **YouTube Data API
v3** and press **Enable**.

## 3. Set up the consent screen

Open **Google Auth Platform** (it may say **OAuth consent screen**) and press
**Get started**.

- App name: anything you'll recognise on Google's consent page.
- Support and contact email: your own.
- Audience: **External**.

Under **Data Access**, add the scope
`https://www.googleapis.com/auth/youtube.upload`. It's the only one the app
asks for. It can upload a video and set its thumbnail, and it can't read,
edit or delete anything on the channel.

## 4. Make the client

Under **Clients**, press **Create client**.

- Application type: **Desktop app**. Other types won't work: the app signs
  in through a redirect to `127.0.0.1`, which Google allows for desktop
  clients only.
- Name: anything.

Press **Create**, then **Download JSON**. Keep that file private: it holds
the client's secret.

## 5. Set the app to "In production"

Under **Audience**, press **Publish app** so the status reads **In
production**.

This matters. While the status is **Testing**, Google lets a sign-in last
[seven days](https://support.google.com/cloud/answer/15549945), and you'd
have to sign in again every week. You don't need Google to verify the app
for your own use; you'll see a warning on the consent page instead (step 6).

## 6. Sign in

From the window: **Settings → Sign in**, with a label for the channel (such
as `main`) and the path to the JSON file.

From a terminal:

```sh
mockingbird-studio youtube-connect --label main --client-file ~/Downloads/client_secret_….json
```

Your browser opens Google's consent page. Because the app isn't verified,
Google shows "Google hasn't verified this app". Press **Advanced**, then
**Go to <your app name>**, then allow the upload permission. The page then
says you can close the tab.

The app waits up to five minutes for this. It keeps the client's id and
secret and the refresh token in the macOS Keychain, under
`mockingbird-studio` and the label. You can delete the JSON file now.

A niche posts to the channel whose label is in its `channel` field.

## Checking and signing out

```sh
mockingbird-studio youtube-status
mockingbird-studio youtube-disconnect --label main
```

`youtube-status` lists each label and whether it's signed in. It never shows
a token. `youtube-disconnect` revokes the token at Google and deletes it
from the Keychain. You can also remove the app's access at
[myaccount.google.com/permissions](https://myaccount.google.com/permissions).

## What Google still limits

- Until your project passes Google's
  [audit](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits),
  videos uploaded through it are locked to private. The app uploads with
  everything filled in, and you press Publish in YouTube Studio.
- Custom thumbnails need a
  [verified channel](https://www.youtube.com/verify).
- Uploads count against your project's
  [daily quota](https://developers.google.com/youtube/v3/getting-started#quota).
