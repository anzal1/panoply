# Everyday utility task harvest (2026-10-13)

Goal: the most complete list of everyday utility TASKS people go to websites or paid apps for, so an on-device app can cover them.
Method: tool index pages fetched live on 2026-10-13, plus Semrush/Ahrefs-derived pages and keyword pages for demand. No em dashes. Numbers are quoted as found; anything I inferred is marked "est".

## 0. Source coverage and honesty notes

Site abbreviations used below:
iLP = iLovePDF, iLI = iLoveIMG, SP = Smallpdf, P24 = PDF24 Tools, 123 = 123apps, TW = TinyWow, CC = CloudConvert, FC = FreeConvert, OC = Online-Convert, EZ = ezgif, KW = Kapwing, VD = VEED, AE = Adobe Express, PR = Photoroom, IT = IT-Tools, CH = CyberChef, PT = PowerToys, SA = Setapp, RC = Raycast, IN = Indian exam/form resizer sites, BD = Bangladesh form sites.

Harvest status per source:
- iLP, iLI, SP, P24, 123, TW, FC, OC, EZ, KW, AE, PR: full tool menus fetched. VD: fetched but the page was truncated (about 100K of 115K chars read), long-tail language pages not listed.
- IT: the index page is a JS app (empty to a fetcher), so I read the tool registry in the GitHub repo (src/tools/index.ts, 10 categories, about 89 tools).
- CH: category summary from the repo Categories.json (17 categories, counts below). Not per-operation.
- CC: the formats page gives only counts (212 formats, 11 categories; documents named: 23). CloudConvert is a pure format matrix, so it adds no new task types beyond the conversions listed.
- PT: full list of 30 utilities from Microsoft Learn (page dated 2026-09-14).
- Canva free tools: Canva returned HTTP 403 to every fetch. Canva-only tasks are NOT captured. AE and KW overlap most of what Canva offers as free quick tools.
- RC: top 50 by download count compiled from store pages 1 to 5 (see section 5). The store page order inside one page is not strictly sorted.
- SA: Setapp publishes no popularity numbers. I got 5 official categories (Optimize, Work, Create, Develop, Solve with AI+) and example apps, not a ranked top 40. Top 40 by category is not achievable from public pages.
- Large Indian utility sites: I found about 12 exam-resizer sites (list in section 4) but no traffic ranking among them, and Semrush pages for photokb.in and pdf24.org returned 404.

Demand-data caveats:
- Ahrefs pages for pdf24.org, pdf.io, compress2go.com redirect from ahrefs.com to ahrefstop.com, a domain I could not verify as Ahrefs. I used those figures only as "Ahrefs-style, unverified mirror" and the data looks like an older snapshot (it disagrees with the Ahrefs India ranking quoted via search). Treat as order-of-magnitude.
- seodata.dev volumes are US-only monthly. Ahrefs-style mirror volumes appear global. They differ by 10x to 20x for the same keyword (jpg to pdf: 201,000 US vs 4.8M global). Never compare across the two.
- Semrush country shares show Bangladesh as the top country for most of these sites in Aug/Sep 2026 (up to 43% for remove.bg). That is surprising and could partly reflect Semrush panel or bot effects. Direction (South Asia is huge) is consistent with the earlier Dec 2025 snapshot where India led ilovepdf at 22.21%.

## A. Master task list

Format: task | sites. 457 numbered entries (est: roughly 330 distinct after merging near-duplicates such as per-format converters). Tasks are grouped by user goal, not by site.

### A1. PDF (53)
1. Merge PDFs | iLP SP P24 123 TW FC
2. Split PDF into files or ranges | iLP SP P24 123 TW FC
3. Compress PDF | iLP SP P24 123 TW FC OC
4. Remove (delete) pages | iLP SP P24 123
5. Extract pages | iLP SP P24 123 FC
6. Rearrange / organize pages | iLP SP P24 123 TW FC
7. Rotate pages | iLP SP P24 123 FC
8. Crop PDF | iLP SP FC EZ
9. Resize PDF page size | FC
10. Edit PDF text and images | iLP SP P24 TW
11. Annotate / markup PDF | SP FC
12. Fill PDF forms | iLP SP
13. Create fillable PDF form | FC
14. Sign PDF yourself (draw, type, image) | iLP SP P24 TW FC
15. Request e-signatures from others | FC TW
16. Redact PDF | iLP SP P24
17. Add watermark | iLP SP P24
18. Add page numbers | iLP SP P24 123
19. Password-protect PDF | iLP SP P24 123 TW
20. Unlock PDF / remove password | iLP SP P24 123 TW FC
21. Flatten PDF | SP FC
22. Compare two PDFs | iLP P24
23. Repair corrupted PDF | iLP
24. OCR PDF (make searchable) | iLP SP P24
25. PDF to PDF/A | iLP SP
26. Web-optimize (linearize) PDF | P24
27. PDF overlay / background merge | P24
28. Extract images from PDF | P24 FC
29. Extract text from PDF | TW
30. PDF reader / viewer | SP FC
31. PDF to Word | iLP SP 123 TW FC OC P24
32. PDF to Excel | iLP SP 123 TW
33. PDF to PowerPoint | iLP SP TW
34. PDF to JPG / PNG | iLP SP P24 123 TW FC OC
35. PDF to HTML | 123
36. PDF to Markdown | iLP
37. PDF to EPUB | FC
38. Word to PDF | iLP SP 123 TW FC OC
39. PowerPoint to PDF | iLP SP 123
40. Excel to PDF | iLP SP 123 TW
41. JPG / PNG / images to PDF | iLP SP P24 123 TW FC OC
42. HTML / webpage to PDF | iLP SP P24 OC
43. TXT / RTF / ODT / ODP / ODS / HWP / Pages to PDF | SP
44. EPUB to PDF | SP FC
45. ZIP or CSV to PDF | SP
46. Scan with camera to PDF | iLP SP
47. Create blank or new PDF | P24 TW
48. Chat with PDF (ask questions) | SP
49. Summarize PDF | iLP SP
50. Translate PDF | iLP SP TW
51. Generate quiz questions from PDF | SP
52. PDF to audio (read aloud) | VD
53. PDF or PPT to video | VD

### A2. Image edit and convert (78)
54. Compress image | iLI TW FC OC 123 KW
55. Resize by pixels or percent | iLI TW FC 123 KW AE PR PT
56. Resize image to target KB | TW IN (see A8)
57. Resize for social platform sizes | PR KW AE
58. Bulk resize many images | PR PT
59. Crop image | iLI TW FC AE
60. Rotate image | iLI FC KW
61. Flip / mirror image | TW FC KW
62. Convert any image to JPG | iLI AE PR OC
63. Convert JPG to PNG / GIF / other | iLI FC KW AE PR
64. WebP to JPG / PNG | FC TW EZ KW OC
65. JPG / PNG to WebP | KW FC
66. AVIF to JPG / PNG and back | EZ
67. HEIC to JPG / PNG / PDF | FC TW OC KW
68. RAW to JPG | EZ
69. BMP / TIFF / TGA / EPS / HDR converters | OC EZ
70. PNG / JPG to SVG (vectorize) | FC AE PR OC
71. Image to ICO / favicon | OC EZ
72. Image to Data URI / base64 | EZ IT
73. Upscale / increase resolution | iLI TW FC KW VD
74. Unblur / sharpen photo | TW KW
75. Enhance / brighten photo | PR KW
76. Restore old photos | TW KW
77. Colorize black and white photo | TW KW
78. Remove background | iLI TW FC AE PR KW VD
79. Change background / replace | TW AE PR KW
80. Solid white or color background (for products, IDs) | PR
81. Blur background | TW PR KW
82. Transparent background maker / PNG maker | TW PR KW
83. Blur face / censor / pixelate | iLI TW KW
84. Remove object | TW PR KW
85. Remove person | TW
86. Remove text from image | TW KW
87. Remove watermark from image | TW KW
88. General photo editor (filters, brightness, saturation) | iLI KW
89. Add text to image | TW PR KW AE
90. Curved text | PR
91. Add border / frame | TW KW
92. Add sticker | PR KW
93. Add shadow / outline | PR KW
94. Round / circle crop avatar | TW PR
95. Profile picture maker | TW PR
96. Meme generator | iLI TW 123 KW
97. Photo collage / grid | TW 123 KW
98. Combine / merge images side by side | TW KW
99. Split image into grid pieces | TW
100. Add image watermark (logo) | iLI KW
101. Image to text (OCR) | TW OC PT
102. Translate text inside image | KW
103. Pick color from image or screen | 123 PT RC
104. Color palette / color converter (HEX, RGB, HSL) | IT PT
105. View / strip image metadata (EXIF) | EZ
106. Invert colors | KW
107. Black and white / sepia filter | TW PR KW
108. Cartoonify / style filters | KW
109. AI image generation | TW KW VD PR
110. Chart maker | TW
111. Screenshot beautify / annotate | SA RC
112. Measure pixels on screen | PT
113. Signature maker (typed, drawn, handwritten) | PR (see A8)
114. Logo maker | PR AE
115. Icon maker | PR
116. Poster / birthday card maker | PR
117. Phone wallpaper / banner maker | KW
118. YouTube thumbnail maker | KW
119. WhatsApp / messaging sticker maker | PR KW
120. Image to video | KW VD
121. Image to GIF | 123 EZ KW
122. Website screenshot to image or PDF | OC
123. SVG placeholder generator | IT
124. Product photo staging / shadows | PR
125. Recolor clothing / products | PR
126. Ghost mannequin | PR
127. Virtual try-on | PR
128. Image extender / outpainting | PR KW VD
129. Inpaint / fill selected area | KW
130. Image to image AI restyle | VD
131. Drawing / doodle on image | KW
132. Duplicate image | KW

### A3. Photo ID and forms (see also section C) (14)
133. Passport size photo maker (country specs) | IN BD TW
134. Visa photo / ID photo maker | IN BD
135. Exam form photo resize (KB and pixel limits) | IN BD
136. Signature resize / crop to KB and pixels | IN
137. Thumb impression resize | IN
138. Combined photo + signature in one strip | IN
139. Add name and date strip under exam photo | IN
140. Handwritten declaration resize | IN
141. PAN card photo and signature resize | IN
142. Passport Seva upload photo (630x810) | IN
143. Photo/doc under 100 KB or 200 KB PDF | IN
144. Image to PDF with size cap | IN
145. Print layout of passport photos on 4x6 sheet (est, common feature) | IN
146. Scan signature on paper to clean digital PNG | IN

### A4. Video (62)
147. Compress video | TW 123 FC KW VD OC
148. Trim / cut video | TW 123 FC AE KW VD
149. Merge / join videos | 123 KW VD AE
150. Crop video | 123 FC AE KW VD
151. Rotate video | 123 EZ KW VD
152. Flip video | 123 KW VD
153. Resize / reframe aspect ratio | TW 123 AE KW VD
154. Change speed | 123 AE KW VD EZ
155. Reverse video | EZ KW
156. Loop video | 123 KW VD
157. Mute video / remove audio | TW KW VD EZ
158. Add audio / music to video | 123 KW VD
159. Replace audio track | KW
160. Extract audio from video | TW KW VD EZ
161. Change video volume | 123 VD
162. Stabilize video | 123 KW
163. Add text / titles | 123 KW VD
164. Auto captions / subtitles | AE KW VD
165. Edit subtitles | KW VD
166. Convert / translate subtitles (SRT, VTT) | KW VD
167. Sync subtitles | KW
168. Burn subtitles into video (SRT to MP4) | KW
169. Add image / overlay to video | 123 KW VD
170. Add watermark to video | KW
171. Remove logo / watermark from video | 123 VD
172. Blur / censor part of video | KW VD
173. Remove video background / green screen | AE KW VD
174. Add filters and effects | KW VD
175. Add transitions / crossfade | KW VD
176. Split-screen / collage video | KW
177. Freeze frame, zoom, Ken Burns | KW
178. Video to GIF | TW 123 AE EZ FC
179. GIF to MP4 | 123 EZ FC KW
180. Video to photo / frame grab | VD
181. Convert video format (MP4, MOV, MKV, AVI, WebM, WMV, FLV, 3GP, MPEG, OGV) | TW 123 FC OC AE KW VD CC
182. MOV to MP4 | TW FC KW AE
183. Frame-rate converter | VD
184. Upscale / enhance video | KW VD
185. Denoise video | KW
186. Screen recorder | 123 EZ KW VD SA PT
187. Webcam / video recorder | 123 KW VD
188. Webcam test | KW VD
189. Teleprompter | KW VD
190. Slideshow / photo video maker | KW
191. Intro maker | KW VD
192. Progress bar overlay | KW VD
193. Time lapse / stop motion | KW
194. Video downloader from social links | TW VD RC (needs internet)
195. Video to text / transcript | TW KW VD
196. YouTube / social transcript generator | TW KW VD
197. Dubbing / translate video | KW VD
198. AI lip sync / eye contact | KW VD
199. Voiceover generator | KW VD
200. Video safe-zone guides (TikTok, Reels, Shorts) | KW
201. Clip maker / highlight finder | AE KW VD
202. Animate characters / AI motion | AE KW
203. Text to video / image to video AI | KW VD PR
204. Video background noise removal | VD
205. Video link generator / hosting | KW VD (needs internet)
206. Video filler-word remover | VD
207. Video editor (timeline) | 123 KW VD
208. Compress video for Discord / email | KW

### A5. Audio (38)
209. Convert audio formats (MP3, WAV, FLAC, M4A, OGG, AAC, WMA, OPUS, AIFF, MMF) | 123 FC OC EZ KW CC
210. MP4 to MP3 / video to MP3 | TW EZ FC KW VD
211. M4A to MP3 | TW EZ KW
212. WAV to MP3 / FLAC to MP3 | EZ KW
213. Make ringtone (M4R) | OC
214. Compress audio (MP3, WAV) | FC EZ
215. Trim / cut audio | 123 KW VD
216. Join / merge audio | 123 KW VD
217. Change speed | 123 EZ VD
218. Change pitch | 123
219. Volume change / boost | 123 EZ KW VD
220. Fade in / out | EZ KW
221. Equalizer | 123 KW
222. Reverse audio | 123
223. Loop audio | KW VD
224. Remove silences | KW
225. Remove background noise | KW VD
226. Remove reverb | KW
227. Vocal remover / isolate voice / split stems | KW
228. Voice changer | KW
229. Voice recorder | 123 KW VD
230. Sound / mic test | VD
231. Waveform / music visualizer | EZ KW VD
232. Text to speech | 123 KW VD
233. Speech to text / dictation | TW VD
234. Transcribe audio / podcast | KW VD TW
235. Translate audio | KW VD
236. Audio to video (MP3 to MP4 with image) | KW
237. Podcast editor | KW
238. Filler word remover | VD
239. Sound effects library or generation | KW
240. Audio player | VD
241. AI voice cloning | KW VD
242. AI music / song generator | KW VD
243. AI audio enhancer | KW VD
244. Crossfade audio | KW
245. Duplicate / replace audio in GIF or image | KW
246. Set default audio device quickly (desktop) | RC

### A6. GIF and animation (13)
247. Make GIF from images | EZ 123 KW
248. Resize GIF | EZ KW
249. Crop / rotate GIF | EZ KW
250. Trim / cut GIF | KW EZ
251. Optimize / compress GIF to target size | EZ 123
252. GIF speed / reverse | EZ
253. Add text to GIF | EZ KW
254. Split GIF into frames | EZ
255. WebP / APNG / AVIF animated maker and converter | EZ
256. Lottie to GIF / WebP | EZ
257. GIF analyzer | EZ
258. Screen to GIF | EZ KW
259. GIF search (find reaction GIFs) | RC (needs internet)

### A7. Documents, ebooks, data, archives, fonts (39)
260. DOC / DOCX / ODT / RTF / TXT / HTML conversion | OC FC CC
261. PPT to PPTX and slides conversion | OC CC
262. Excel / CSV / ODS conversion | OC TW CC
263. CSV to Excel / JSON | TW
264. Excel to XML / XML to Excel | TW
265. XML to JSON / JSON to XML | TW IT
266. XML to CSV | TW
267. Split CSV | TW
268. Split Excel | TW
269. Markdown to HTML | IT CC
270. EPUB / MOBI / AZW3 / FB2 / LIT / LRF / PDB / TCR ebook conversion | FC OC TW CC
271. Archive convert (ZIP, 7Z, TAR, RAR, TAR.GZ, TAR.BZ2) | FC OC CC
272. Create ZIP | TW
273. Extract archive | 123
274. Font convert | 123 CC
275. CAD convert | OC CC
276. Device-profile video convert (iPhone, Android, PSP) | OC
277. Vector convert (SVG and others) | CC FC
278. Hash generator for text (MD5, SHA, CRC, Whirlpool, many) | OC IT CH
279. File checksum / verify hash | CH (est)
280. Compress / decompress data (gzip, bzip2, zip) | CH
281. Subtitle file convert | KW VD
282. Bulk file renamer | EZ PT
283. Rename with regex and preview | PT
284. File preview without opening (quick look) | PT
285. New file from template | PT
286. Find what process locks a file | PT
287. YAML / JSON / TOML conversion | IT
288. JSON format / minify / view / diff | IT RC
289. JSON to CSV | IT
290. SQL prettify | IT
291. XML format | IT
292. HTML / XML / JSON beautify and minify | CH
293. Diff two texts | IT CH
294. Docker run to compose | IT
295. Epoch / date-time converter | TW IT
296. Registry file preview | PT
297. Edit hosts file | PT
298. Manage environment variables | PT

### A8. Text and writing (36)
299. Word / character counter | IT RC TW (est) 
300. Text statistics (reading time, frequency) | IT
301. Case converter | IT
302. Lorem ipsum | TW IT RC
303. Grammar fixer | TW
304. Paraphrase / sentence rewriter | TW
305. Summarize text | TW
306. Paragraph / essay / article writer | TW
307. Humanize AI text | TW
308. Tone of voice changer | TW
309. Translate text | RC (Google Translate) KW VD
310. Social captions and posts (IG, LinkedIn, FB headline) | TW
311. FAQ / blog outline / blog ideas generator | TW
312. Business name generator | TW
313. YouTube script writer | TW VD KW
314. Story generator | TW
315. Real estate listing description | TW
316. Slugify string | IT
317. List converter / sort / dedupe lines | IT
318. Emoji picker / search | IT RC
319. Kaomoji search | RC
320. ASCII text art | IT
321. Text to binary / unicode / NATO alphabet | IT CH
322. Numeronym generator | IT
323. String obfuscator | IT
324. Roman numeral converter | IT
325. Dictionary / thesaurus | RC
326. Accent / special character typing | PT
327. Paste as plain text / reformat clipboard (AI optional) | PT SA
328. Markdown / HTML WYSIWYG editor | IT
329. Font generator (fancy text) | KW
330. Animated text maker | KW
331. Regex tester | IT CH
332. Email normalizer | IT
333. Meta tag generator (SEO) | IT
334. Format phone number | IT

### A9. Calculators and converters (27)
335. Unit converter (length, mass, volume) | 123 OC
336. Temperature converter | IT
337. Number base / binary / hex converter | IT CH
338. Percentage calculator | IT
339. Math expression evaluator | IT
340. Age calculator (demand list, not in harvested menus) | est
341. EMI / loan calculator (demand list, not in harvested menus) | est
342. BMI calculator (demand list) | est
343. GST / SIP / tax calculators (India, not verified in menus) | est
344. Currency converter (needs rates) | est, needs internet
345. Time converter / time zone | 123 IT
346. ETA calculator | IT
347. Stopwatch / chronometer | IT
348. Countdown timers | RC
349. Pomodoro | RC
350. Benchmark / timing builder | IT
351. Random number / password / token generator | IT
352. Password strength checker | IT
353. UUID / ULID generator | IT
354. Chmod / crontab helper | IT
355. IPv4 subnet calculator | IT
356. IBAN validator | IT
357. Keycode info | IT
358. Screen ruler / pixel measure | PT
359. Sound / mic level test | VD
360. Speed test (internet) | RC (needs internet)
361. Battery / system monitor | RC SA

### A10. Security, crypto, network (31)
362. Hash text (MD5, SHA, bcrypt) | IT CH OC
363. Encrypt / decrypt text | IT CH
364. Password / token generator | IT
365. BIP39 mnemonic generator | IT
366. HMAC generator | IT
367. RSA key pair generator | IT CH
368. OTP / TOTP code generator and validator | IT
369. 2FA code finder | RC
370. JWT parser | IT
371. Base64 string / file | IT RC CH
372. URL encode / decode / parse | IT CH
373. HTML entities | IT
374. Basic auth header generator | IT
375. User-agent parser | IT
376. Device info viewer | IT
377. HTTP status code reference / MIME types | IT
378. Safelink decoder | IT
379. PDF signature checker | IT
380. QR code generator (URL, text, WiFi) | TW IT AE EZ
381. QR / barcode reader (scan) | est
382. Barcode generator | EZ
383. MAC address lookup / generator | IT
384. IPv6 ULA generator | IT
385. Random port generator / port manager | IT RC
386. Extract IPs, emails, URLs from text | CH
387. Forensics (magic detect, file carving, entropy) | CH
388. Public-key ops (PGP, X.509, JWT) | CH
389. Compression ops (gzip, bzip2, zip) | CH
390. Multimedia ops (image/hex/audio dumps) | CH
391. Network ops (IP, UDP, HTTP) | CH
392. Fork / merge flow-control recipes (chain ops) | CH

### A11. Desktop and productivity utilities (PT, SA, RC) (42)
393. Window layouts / snap zones | PT SA
394. Always on top | PT
395. Keep awake / caffeinate | PT RC
396. Color picker | PT RC
397. Screen text extractor (OCR from screen) | PT SA
398. Bulk image resizer from file explorer | PT
399. Keyboard remap and shortcuts | PT
400. Shortcut cheat sheet | PT
401. App launcher / command palette | PT RC
402. Mouse highlighter / crosshair / find mouse | PT
403. Share one mouse across computers | PT
404. Monitor brightness and input control | PT
405. Light / dark theme schedule | PT
406. Workspace launcher (open set of apps) | PT
407. Crop and lock window thumbnail | PT
408. Peek file preview | PT
409. Screen zoom and annotate | PT
410. Clipboard manager / history | SA
411. Kill process | RC
412. Disk cleaner / uninstaller | SA RC
413. Menu bar organizer | SA
414. Battery health / charge limit | SA
415. System monitor | SA RC
416. Screenshot and screen record | SA RC
417. Downloads manager | RC
418. Timers | RC
419. Calendar / reminders / to-do | SA RC
420. Notes | SA RC
421. Password manager lookup | RC
422. Bookmark search | RC
423. Remove paywall / reader mode | RC (needs internet)
424. Clean keyboard (lock input) | RC
425. Package manager UI (brew) | RC
426. Media control and music player | RC
427. Window switcher between same-app windows | PT
428. Emoji / GIF search | RC
429. Environment / port / process manager | PT RC
430. Quick note to AI (chat with a model) | RC SA
431. Image modification quick actions | RC
432. Git / repo tools | RC SA
433. Dev API client / DB client | SA
434. Mail client | SA
435. Time tracker | SA

### A12. Generative / AI creation (cloud unless noted) (22)
436. Text to image | KW VD TW PR
437. Text to video | KW VD
438. AI avatars / talking photo | KW VD
439. AI headshots | KW
440. AI face swap | KW
441. AI aging | KW
442. AI product photography | PR
443. AI fashion models | PR
444. AI background generation | KW PR
445. AI object remove / insert | PR
446. AI voice generation | KW VD
447. AI music generation | KW VD
448. AI script / ad generation | KW VD
449. AI UGC ads | VD
450. AI B-roll generation | KW VD
451. AI presentation maker | VD
452. AI humanizer / paraphraser | TW
453. AI storyboard | KW
454. AI logo | KW PR
455. Chat with assistant | RC
456. AI summarizer for video, page, PDF | TW SP iLP
457. AI doc to video | KW

## B. Top 40 tasks by demand evidence

Ranked by highest single volume found, then by usage evidence. Units differ by row, see notes.

| # | Task | Strongest numbers found | Source and caveat |
|---|---|---|---|
| 1 | PDF to Word | 4.0M/mo (global-style, Ahrefs mirror); 246K US; 246K Bangladesh DB; MiOffice rank 15 | ahrefstop mirror (pdf.io), seodata.dev, Semrush smallpdf Sep 2026 |
| 2 | JPG to PDF | 4.8M/mo (global-style); 201K US; 246K Bangladesh; MiOffice rank 14 | same |
| 3 | PDF to JPG | 3.4M/mo (global-style); 165K US; 246K Bangladesh | same |
| 4 | BMI calculator | 2.74M/mo US | seodata.dev 2026-05-11 |
| 5 | Word to PDF | 2.6M/mo (global-style); 110K US; 135K Bangladesh | same |
| 6 | YouTube to MP3 | 2.24M/mo US (368K "yt to mp3") | seodata.dev 2026-07-22 |
| 7 | Merge PDF | 1.7M/mo (global-style); 135K US; MiOffice rank 9 | same |
| 8 | Compress PDF | 1.6M/mo (global-style); 110K US; MiOffice rank 2 (1,710 uses/30d) | same |
| 9 | Resize image | 1.22M/mo India (iLoveIMG snapshot); 110K US ("image resizer"); MiOffice rank 3 | Semrush iloveimg Aug 2026; seodata |
| 10 | PDF converter (generic) | 1.22M/mo US | Semrush freeconvert Aug 2026 |
| 11 | Word counter | 1.22M US (792K Ahrefs-style) | seodata.dev; ahrefs wordcounter.net snippet |
| 12 | Image to PDF | 896K (global-style); 60.5K US "images to pdf" | mirror; seodata |
| 13 | Remove background | 823K US ("remove bg"); 450K Bangladesh DB; MiOffice rank 5 (867 uses) | seodata; Semrush remove.bg Sep 2026. Another site claimed 2.5M (indexator), 247K (Ahrefs-style clippingmagic); sources disagree |
| 14 | Reduce image size / photo compressor | 823K India ("photo compressor" and "reduce image size"); 368K India "compress image" | Semrush iloveimg Aug 2026; resizepixel snapshot |
| 15 | YouTube downloader | 823K US; 550K "youtube video download" | seodata.dev 2026-07-24 |
| 16 | QR code generator | 673K to 823K US | seodata.dev 2026-07-09 |
| 17 | Percentage calculator | 673K US | seodata.dev |
| 18 | Currency converter | 673K US | seodata.dev |
| 19 | Video converter | 673K US | Semrush freeconvert |
| 20 | MP3 converter | 550K US | Semrush freeconvert |
| 21 | Age calculator | 550K US ("calculate age" 450K) | seodata.dev |
| 22 | Split PDF | 336K (global-style) | mirror |
| 23 | Text to speech | 301K US | seodata.dev |
| 24 | HEIC to JPG | 246K US | seodata; Semrush freeconvert |
| 25 | WebP to JPG / PNG | 246K India (webp to jpg); 165K US (webp to png); MiOffice ranks 10 and 12 | Semrush |
| 26 | Passport size photo | "picture for passport" 246K US (as listed); "passport image size" 14.8K US | seodata. The 246K row is odd, treat as unverified |
| 27 | GIF maker | 201K US | seodata |
| 28 | Password generator | 201K US | seodata |
| 29 | PNG to JPG | 60.5K US | seodata |
| 30 | MP4 to MP3 | 165K US; MiOffice "Video to MP3" rank 13 | seodata; MiOffice |
| 31 | Resize image to 20 KB / 50 KB / 100 KB | 90.5K / 60.5K / 49.5K India (est: read from flattened Semrush tables) | simpleimageresizer and resizeimage.io Semrush snapshots 2025 |
| 32 | Video compressor | 90.5K US | seodata |
| 33 | Image to text (OCR) | 90.5K US; 74K Bangladesh DB | seodata; Semrush tinywow |
| 34 | Unit converter | 60.5K US | seodata |
| 35 | Photo collage | 60.5K US (27.1K head term) | seodata |
| 36 | Signature maker / generator | 74K US "signature generator"; 14.8K head term | seodata |
| 37 | Image upscale / increase resolution | 9.9K Bangladesh DB each for two phrasings | Semrush tinywow |
| 38 | M4A to MP3 | MiOffice rank 6 (697 uses/30d) | MiOffice report, own analytics, 35,235 sessions |
| 39 | AI object removal | MiOffice rank 7 (668 uses/30d) | same |
| 40 | EMI calculator | 12.1K US only; India number NOT found (est: far higher in India, unverified) | seodata |

Site-level scale (Semrush, visits per month):
- ilovepdf.com 216.83M (Dec 2025, India 22.21%, Indonesia 6.63%, Brazil 6.21%); 325.13M (Sep 2026, Bangladesh 24.48% = 79.6M, India 14.24% = 46.3M).
- smallpdf.com 56.71M (Sep 2026, Bangladesh 34.96%, India 13.39%).
- remove.bg 105.63M (Sep 2026, Bangladesh 43.35%, India 9.47%).
- canva.com 1.07B (Sep 2026, Bangladesh 21.06%, US 15.28%, India 4.99%).
- iloveimg.com 43.78M (Aug 2026, India 11.97%, Brazil 10.08%, Bangladesh 8.7%).
- freeconvert.com 52.62M (Aug 2026, US 18.14%, Bangladesh 16.36%, India 11.74%).
- cloudconvert.com 40.33M (Aug 2026, Bangladesh 21.5%, US 19.21%).
- tinywow.com 2.52M (Sep 2026, Bangladesh 32.79%).
- Ahrefs India "computers and electronics" ranking (via search snippet, July 2025): ilovepdf.com #1 with about 42M estimated organic search traffic, ahead of apple.com 34M and adobe.com 29M. Pakistan: ilovepdf #9, 1.5M.
- Compress2Go: India 31.2% of 463K visits in April 2025; "compress pdf" about 124K India visits (Semrush via search snippet). CompressJPEG: India about 54% of 2.1M visits (April 2025). pdf.io: India 17.8%, 213K (May 2025).

Usage-event ranking (MiOffice, browser-side tool, Feb 1 to Mar 3 2026, 26,901 events, self-reported, unverified): Image Compress 1,929; PDF Compress 1,710; Image Resize 1,501; Image Convert 1,008; AI Background Removal 867; M4A to MP3 697; AI Object Removal 668; PDF Split 636; PDF Merge 611; WebP to JPG 602; JPG to WebP 564; WebP to PNG 556; Video to MP3 507; JPG to PDF 501; PDF to Word 497.

## C. Region-specific but huge

1. Exam and government form photo and signature sizing to exact KB and pixels (India). Evidence: "resize image to 20kb" about 90.5K/mo India, "50kb" 60.5K, "100kb" 49.5K (est, Semrush snapshots, flattened tables). At least 12 dedicated sites exist (PhotoKB, EasyResizeImage, SignResizer, JobAndExam, ExamToolkit, MyExamPhoto, PortalResizer, OnlineResult, ExamMint, PassportSizePhoto.in, PhotoResizer.co.in, ImageResizer.org.in) plus general sites (Edesy, Codia, imageonline.io). None give traffic numbers I could verify; no site claim of "#1" is evidence.
   Specs vary and conflict across sources, so presets must be user-editable and sourced to the official notification:
   - SSC: photo 20 to 50 KB, signature 10 to 20 KB; photo 3.5 x 4.5 cm (200 x 230 px) most common; signature 140 x 60 px or 6 x 2 cm.
   - UPSC: sources conflict (20 to 300 KB, 20 to 200 KB; 400 x 400 px target).
   - RRB signature: 10 to 40 KB (6 x 2 cm).
   - Indian passport photo 35 x 45 mm. Passport Seva upload: 630 x 810 px, JPEG, 10 to 250 KB, white background (one source, photogov.net; another says 10 to 300 KB, conflict).
   - PAN: NSDL photo 2.5 x 3.5 cm, 50 KB cap, signature 4.5 x 2.0 cm (single personal handbook); UTIITSL 213 x 213 px under 30 KB photo, 400 x 200 px signature under 60 KB; one tool site reports a spec change in April 2026, unconfirmed.
   - KB ambiguity: some portals count 1 KB as 1000 bytes, some as 1024. Target 20,000 bytes to be safe (Edesy FAQ).
   - Related tasks: thumb impression, handwritten declaration, combined photo+signature strip, date and name strip on exam photo, date stamp, PDF under 100/200 KB.
2. Bangladesh form photo and ID sizing. Photobip (Teletalk job portals): photo 300 x 300 px, signature 300 x 80 px, under 100 KB. NID photo 30 x 30 mm (354 x 354 px at 300 DPI). Passport 40 x 50 mm (472 x 591 px at 300 DPI) per several sites, but another source claims 25 x 30 mm for e-passport online and 45 x 55 mm printed; conflict, confirm per form. Bangladesh is now the top traffic country for most PDF/image tool sites per Semrush (Sep 2026).
3. General PDF/image toolbox demand skewed to South Asia and Southeast Asia: ilovepdf top countries India, Bangladesh, Indonesia, Brazil, Colombia. Bangladesh Semrush keywords: "i love pdf" 246K, "jpg to pdf" 246K, "pdf to word" 246K, "remove bg" 450K.
4. India finance calculators (EMI, GST, SIP, PAN-related). Not verified numerically here (US EMI 12.1K only). Marked est.
5. Other region pass: Indonesia, Brazil, Vietnam, Thailand, Turkey, Russia appear in top-5 countries for several tool sites (Ahrefs mirror, Semrush). Localization (Bahasa, Portuguese, Vietnamese, Thai) matters but task types are the same.
6. Not verified: Aadhaar, DigiLocker, Pakistan CNIC photo specs. Check official sites before presets.

## D. Tasks that inherently need the internet

Mark these "online" in the app. Everything else in A is feasible fully on-device (OCR and background removal via local models are feasible; heavy generative models are cloud unless a small local model is acceptable).
1. Downloaders: YouTube, TikTok, Instagram, Facebook, Twitter/X, Twitch, Vimeo, Dailymotion (TW, VD, OC, RC "Video Downloader" 136,511 downloads). Also YouTube subtitles downloader, YouTube transcript, Youtube to text (TW, KW). Legal and platform-policy risk too.
2. Currency converter (live rates).
3. Link shorteners and link-in-bio (not present in the harvested menus; named in the brief, so est).
4. Video link generator / hosting / sharing / collaboration (KW, VD).
5. Cloud translation: Translate PDF (iLP, SP, TW), subtitle translator, audio translator, video translator, dubbing, Google Translate (RC 500K downloads). Local NMT is possible but weaker.
6. Cloud AI generation: text to image/video/music/voice, avatars, face swap, AI headshots, product photography, virtual try-on, voice cloning, lip sync, AI writer tools (KW, VD, TW, PR).
7. AI summarize, chat with PDF, question generation (SP, iLP, TW) unless an on-device model is used.
8. Website to PDF / website screenshot (OC, iLP, SP, P24): needs fetching a URL.
9. Network lookups: MAC address vendor lookup (IT), speed test (RC), IP/WHOIS/DNS style lookups (CH networking, est), safelink decoder resolving redirects (IT).
10. E-sign requests to other signers (FC, TW): needs a server to deliver and track.
11. Remove paywall / reader mode (RC 87,470 downloads).
12. Stock/GIF/emoji search libraries (GIF Search 108,488 downloads) and sound-effect libraries.
13. Cloud sync services: calendar, mail, notes, to-do, password-manager lookup (RC, SA).
14. Live sharing of files over a link, file hosting (est).
15. Font and template libraries when streamed from servers (est).

## E. Raycast Store: top 50 by download count

Compiled from store pages 1 to 5 (counts as displayed). Utility-like items in bold.
1 **Kill Process** 779K; 2 **Color Picker** 619K; 3 Google Chrome 573K; 4 **Google Translate** 500K; 5 Spotify Player 493K; 6 VS Code 414K; 7 Linear 404K; 8 Slack 337K; 9 Brew 296K; 10 Notion 282K;
11 ChatGPT 262K; 12 Arc 253K; 13 1Password 253K; 14 GitHub 211K; 15 **Speedtest** 187K; 16 Obsidian 177K; 17 Apple Notes 156K; 18 **Coffee** 153K; 19 Google Search 152K; 20 **Video Downloader** 137K;
21 **Timers** 124K; 22 Apple Reminders 123K; 23 **System Monitor** 122K; 24 **CleanShot X** 121K; 25 Zoom 117K; 26 Warp 117K; 27 **Pomodoro** 113K; 28 **Lorem Ipsum** 112K; 29 YouTube 112K; 30 **GIF Search** 108K;
31 **Format JSON** 102K; 32 Music 102K; 33 Todoist 97K; 34 **Remove Paywall** 87K; 35 Google Gemini 86K; 36 **Downloads Manager** 86K; 37 Tailwind CSS 81K; 38 **Clean Keyboard** 80K; 39 **Set Audio Device** 77K; 40 Shell 77K;
41 **Emoji Search** 73K; 42 Google Calendar 72K; 43 **Image Modification** 71K; 44 Perplexity 69K; 45 Jira 68K; 46 Browser Bookmarks 68K; 47 Bitwarden 67K; 48 **Port Manager** 63K; 49 Deepcast 63K; 50 OpenAI GPT 62K.
Further down (page 9): **Word Count** 42,668, **Base64** 42,587, 2FA Code Finder 40,211, Easy Dictionary 40,189.
Lesson: the on-device utility tail that people install repeatedly is small and mechanical (kill process, color picker, translate, speedtest, caffeinate, timers, clipboard-ish text transforms, image tweaks).

## F. Setapp (what was obtainable)

Categories: Optimize, Work, Create, Develop, Solve with AI+.
Example apps by category from the catalog page: Optimize (CleanMyMac, Bartender, iStat Menus, AlDente Pro, App Tamer, Lungo, QuitAll); Work (NotePlan, Craft, Spark Mail, Ulysses, Timing, BusyCal, 2Do, GoodTask, Paste); Create (CleanShot X, Xnapper, Gifox, Sip, PixelSnap, Permute, Squash); Develop (TablePlus, DevUtils, Dash, Proxyman); AI+ (TypingMind, Soulver, BoltAI, PDF Pals, TranscribeX). "Hot" badge: CleanMyMac, Bartender, CleanShot X, Paste, TextSniper, AlDente Pro, ClearVPN, Nitro PDF Pro, Spark Mail. No ranked top 40 exists publicly.

## G. Other tool-menu snapshots (raw)

- CyberChef categories (operations): Data format 80, Encryption/Encoding 95, Public Key 33, Arithmetic/Logic 31, Networking 38, Language 7, Utils 52, Date/Time 10, Extractors 20, Compression 21, Hashing 50, Code tidy 30, Forensics 12, Multimedia 29, Other 22, Flow control 10 (source: Categories.json in gchq/CyberChef).
- IT-Tools categories: Crypto 11, Converter 20, Web 16, Images and videos 4, Development 14, Network 6, Math 3, Measurement 3, Text 7, Data 2 (src/tools/index.ts).
- PowerToys (30 utilities, Microsoft Learn 2026-09-14): Advanced Paste, Always On Top, Awake, Color Picker, Command Not Found, Command Palette, Crop And Lock, Environment Variables, FancyZones, File Explorer add-ons, File Locksmith, Grab And Move, Hosts File Editor, Image Resizer, Keyboard Manager, Light Switch, Mouse utilities, Mouse Without Borders, New+, Peek, Power Display, PowerRename, PowerToys Run, Quick Accent, Registry Preview, Screen Ruler, Shortcut Guide, Text Extractor, Window Hopper, Workspaces, ZoomIt.
- CloudConvert: 212 formats, 11 categories (archives 39, audio 21, CAD 3, ebooks 22, fonts 5, images 42, slides 11, spreadsheets 8, vector 10, video 28, documents 23 named: abw, djvu, doc, docm, docx, dot, dotx, html, hwp, hwpx, lwp, md, odt, pages, pdf, rst, rtf, sdw, tex, txt, wpd, wps, zabw).

## Sources

Tool menus: ilovepdf.com, iloveimg.com, smallpdf.com/pdf-tools, tools.pdf24.org/en, 123apps.com, tinywow.com, freeconvert.com, online-convert.com, ezgif.com, kapwing.com/tools, veed.io/tools, adobe.com/express/feature/image and /video, photoroom.com/tools, github.com/CorentinTh/it-tools (src/tools/index.ts), github.com/gchq/CyberChef (Categories.json), learn.microsoft.com/en-us/windows/powertoys, raycast.com/store and /store/popular/2..5,9, setapp.com/apps, cloudconvert.com/formats.
Demand: semrush.com/website/{ilovepdf.com, smallpdf.com, remove.bg, tinywow.com, freeconvert.com, cloudconvert.com, iloveimg.com, canva.com}/overview; seodata.dev/keyword/* (US volumes, May to Aug 2026); ahrefstop.com/websites/{pdf24.org, pdf.io, compress2go.com, india/computers-and-electronics} (mirror of ahrefs.com URLs, unverified); canary.mioffice.ai/blog/2026-file-processing-report; Semrush snapshots of simpleimageresizer.com, resizeimage.io, resizepixel.com (via search); Google India Year in Search 2025 (no utility queries listed).
Region: photokb.in, easyresizeimage.com, signresizer.com, jobandexam.com, examtoolkit.in, myexamphoto.in, portalresizer.in, passportsizephotoratio.in, photogov.net (Indian passport), mergeimages.net (PAN, Passport Seva, Bangladesh), visafoto.com, passport-photo.online, cutout.pro, photobip.com, edesy.in.
