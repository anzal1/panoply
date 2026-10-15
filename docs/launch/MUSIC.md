# Launch film, music shortlist

Brief: a real, human-made track, confident and hard, with a build into a drop, that is licensed for a free
open-source project's promo video posted on X (and later YouTube and Instagram), with no attribution line
needed and no takedown risk worth worrying about.

Researched on 2026-10-15. Every track page below was fetched and the title, artist, length, license line and
badges read from the page text. Two pages (Millenia and Don't Talk) were fetched a second time by hand to
confirm.

## What is not known yet, said plainly

1. **BPM is not listed on any of these pages.** Pixabay does not publish it. A BPM here would be a guess, so the
   column says "not listed". The film's timing is in beats and re-times to any track from 120 to 150 BPM
   (`tools/cues.mjs`). The real BPM comes from the beat analyzer once a track is chosen.
2. **Nobody has listened to these.** The "why it fits" lines come from the page's genre, mood and tags, not from
   audio. A tag saying "Beat Drop" is the artist's own label. Please audition the top three before picking.
3. **Human-made is likely, not proven.** None of the six carries Pixabay's "AI generated" label, and each has a
   verifiable human signal (the artist's external music links, or a publish date before 2023). Pixabay's own FAQ
   says a missing label does not prove a human made it. The AI-labelled tracks that exist on the site (for example
   "Bass House" by Aurec and the "Banger" tracks) were skipped on purpose.

## The six

License for every row: Pixabay Content License, https://pixabay.com/service/license-summary/. The key line is
"Use Content without having to attribute the author". It does not allow selling or distributing the track on its
own, which does not apply to a video that uses it. The license page does not name X, and it does not restrict
promotional use of a track inside a video. Every track page says "Free for use under the Pixabay Content License".

Content ID badge means the artist registered the track with YouTube's claim system. It does not stop you using it
and does not affect X in practice (see "Risk"), but it can trigger an automatic claim on a YouTube or Instagram
upload that you clear with one click. No-badge tracks avoid even that.

| # | Title | Artist | Length | BPM | Content ID badge | Link |
|---|---|---|---|---|---|---|
| 1 | Millenia | DeltaX-Music | 1:42 | not listed | no | https://pixabay.com/music/beats-millenia-413782/ |
| 2 | Dance For Me | DeltaX-Music | 2:10 | not listed | no | https://pixabay.com/music/dance-dance-for-me-280006/ |
| 3 | Darkness | DeltaX-Music | 2:13 | not listed | no | https://pixabay.com/music/dubstep-darkness-462142/ |
| 4 | Don't Talk | Cosmonkey | 1:51 | not listed | yes | https://pixabay.com/music/future-bass-dont-talk-315229/ |
| 5 | Energy Trap Bass | AleXZavesa | 1:20 | not listed | yes | https://pixabay.com/music/trap-energy-trap-bass-111291/ |
| 6 | [Intro Music] BLACK BOX, Trap Beat | BLACKBOX | 1:57 | not listed | no | https://pixabay.com/music/beats-intro-music-black-box-trap-beat-13221/ |

Why each fits, from the page's own tags:

1. **Millenia.** Tags include Hard and Beat Drop, plus Trap, EDM and Dubstep, genre Future Bass. The only no-badge
   track with an explicit drop tag. The mood tags lean bright (Laid Back, Uplifting), so the baddie edge is the
   thing to check by ear. 1:42 trims cleanly to the 37.5 s film. Artist describes himself as an LA record producer
   with Spotify, Apple Music and YouTube links.
2. **Dance For Me.** Energetic and Restless, tags Intense, Club Music, Seductive, with vocals. The strongest "baddie"
   signal and no badge. No drop tag, so check whether the build is real. The vocal may fight the on-screen text.
3. **Darkness.** Dubstep and EDM, Restless and Energetic, instrumental, no badge. A dubstep drop is likely but
   unconfirmed. The tags are generic (rave, festival).
4. **Don't Talk.** The hardest tag set: Trap, Hard, Aggressive, Punchy, Powerful, Street. Future Bass genre. Over a
   million plays. No explicit drop tag. Has the badge, and the artist lists a claims contact on the page.
5. **Energy Trap Bass.** Trap, Hard, Aggressive, Heavy, Punchy. Shortest at 1:20, so the easiest to cut. Badge, no
   drop tag, published in 2022.
6. **BLACK BOX, Trap Beat.** The description says energetic instrumental trap with punchy 808 basslines. Published
   in January 2022, before AI music flooded the site. No drop confirmed. The page asks for credit, which is a request
   and not a condition of the license.

Swap-ins if the first six do not sound right. All no-badge, and all tagged Beat Drop on their pages:

| Title | Artist | Length | Link |
|---|---|---|---|
| Welcome 2 The Party | DeltaX-Music | 2:23 | https://pixabay.com/music/techno-trance-welcome-2-the-party-452360/ |
| Perfect Storm | DeltaX-Music | 2:46 | https://pixabay.com/music/dance-perfect-storm-386892/ |
| The Way | DeltaX-Music | 2:35 | https://pixabay.com/music/pop-the-way-301056/ |

Also checked: Cosmonkey "So Fresh" (1:37, badge), Gvidon "Spinning Head" (2:08, badge, vocal hook), DeltaX "Bay Area
Connect" (2:49, drum and bass).

## The pick

1. **Millenia** first: no badge, explicit drop tag, a verifiable human producer, and the right length.
2. **Don't Talk** if Millenia sounds too bright, because its tags are the hardest. Cost is the badge.
3. **Dance For Me** if you want a vocal, club, confident feel.

The short version of the taste test: play the first 30 seconds of the top three with the sound on and ask if the
drop lands where a hotkey press should land. If yes, that is the track.

## Risk, honestly

- **X.** I found no sign that X fingerprints audio the way YouTube does. Its approach is reactive: it takes a post down
  after a valid copyright notice. That comes from secondary sources, because X's own help page was not readable. For a
  properly licensed Pixabay track the realistic X risk is low. It is not zero, because Pixabay itself says contributors
  sometimes register tracks with claim systems, and a rights holder can still file a notice.
- **YouTube and Instagram.** Pixabay's FAQ says some tracks are registered with YouTube's Content ID, and a claim is
  "not a copyright strike". If a claim shows up, the dispute is "I have permission or a license", with the track URL and
  the license summary page. A rejected second appeal could escalate to a strike, which is why no-badge tracks are the
  first picks.
- **Keep the proof.** Save the track page URL, the license summary URL and a dated screenshot of both in
  `docs/launch/music-proof/` when the track is chosen. That is what you send if anyone asks.
- **Not checked.** DeltaX distributes on Spotify and Apple Music, so copies of the same tracks may sit in other rights
  databases that Pixabay does not mark. Nobody has checked that.

## Sources that were ruled out, and why

| Source | Why not |
|---|---|
| Uppbeat free tier | Its pages blocked the fetch (HTTP 429), so the terms were not read from the source. Third-party summaries say the free tier needs a credit in the video description, is for personal channels and not for commercial promo, and caps downloads. A description credit does not work on X. |
| YouTube Audio Library | The support page says YouTube cannot give legal guidance on use outside YouTube. Nothing in it allows posting on X. |
| NCS (NoCopyrightSounds) | The usage policy covers YouTube and Twitch, needs a credit in the description, and tells brands promoting their own products to ask for a written license. Great drops, wrong terms. |
| Mixkit | Free, no credit, online ads allowed, but the full license text and the track pages could not be fetched, so no track was verified. Its music page forbids use in CDs, DVDs, video games and TV or radio. Worth a manual look. |
| Free Music Archive, Incompetech, FreePD | No CC0 or CC BY track in the right genre was confirmed. |

One note on the research itself: a Mixkit info page contained text addressed to AI assistants telling them how to
describe Mixkit's legal safety. It was ignored and the claims above come only from the license and policy pages.

## If you already subscribe to Artlist or Epidemic Sound

Both have better "baddie" catalogs, but the plan matters for X:

- **Artlist** (https://help-center.artlist.io/privacy-terms/artlist-license). The Social license lists YouTube, Facebook,
  Instagram, TikTok, Twitch and podcasts, not X, bars paid media, and is for personal channels. The Pro and Business
  license lists Twitter, commercials and paid and promoted videos, so only that tier covers X. Monetized use needs your
  channel URLs on the clearlist or you can get claims.
- **Epidemic Sound** (https://www.epidemicsound.com/blog/which-plan-is-right-for-you/). The plan table does not name X.
  Creator covers social media, podcasts and web, and marks business and client work as not included. Pro and Business add
  digital ads. Confirm your tier with them before posting a product promo.

## If you want a specific commercial hit

Then you must license it, or accept the risk and add it yourself. Plainly:

- To license a major-label song for a promo on X you need a sync license from the publisher and a master license from the
  label. For a hit that is a negotiated, paid deal, not a subscription click, and it is rarely cleared for a company-style
  product promo at indie prices.
- Without a license, X can mute the audio or remove the post after a notice, and a notice against the account is a mark on
  it. Instagram and YouTube are worse because they fingerprint automatically.
- If you accept that risk, do it outside this repo. The film's render is silent-ready: export the picture without audio
  (the master is rendered silent anyway, see "Audio" in PRODUCTION.md), lay the track in an editor, and post it from your own account. The
  picture is cut in beats, so only the cue sheet needs the hit's real BPM and drop.
- Do not commit a track you do not have the rights to into this public repo. This also applies to the free ones: the
  Pixabay license bars distributing the track as a standalone file, and a public repo is exactly that. The track stays
  out of git; only the finished film, with the track inside it, is ever published.

## What happens next

1. You listen and pick one.
2. With your yes, the track's preview file is downloaded into `docs/launch/video/assets/` locally (a few MB, that folder is gitignored) or the
   full file is saved from the track page by you if the site asks for a click-through. The analyzer then measures the real
   BPM, downbeats and drop (`analyze-beatgrid.py`), and `tools/cues.mjs` writes the real cue sheet. Nothing is downloaded
   until you say so.
3. The proof screenshots go in `docs/launch/music-proof/`.
