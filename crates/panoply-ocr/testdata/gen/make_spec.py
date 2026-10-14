"""Writes the render spec and the label files of the generated text set."""
import sys

SPEC = [
    # id, font, size in points, width in pixels at 300 DPI, text
    ("en-1", "Times New Roman", 12, 1700,
     "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. "
     "How vexingly quick daft zebras jump! The five boxing wizards jump quickly. "
     "A wizard's job is to vex chumps quickly in fog."),
    ("en-2", "Helvetica Neue", 11, 1700,
     "Optical character recognition converts pictures of printed text into characters that a "
     "computer can search and copy. Its accuracy depends on resolution, contrast and the "
     "cleanliness of the scan, and it is usually measured as the character error rate against "
     "a text that a person has typed from the same page."),
    ("en-3", "Courier New", 11, 1700,
     "Invoice 2026-0417 was issued on 14 March 2026 for a total of $1,284.50, due within 30 "
     "days. Questions? Write to billing@example.com or call (555) 013-2276 between 9:00 and 17:30, "
     "Monday to Friday. Reference code: AB-7731/Q4."),
    ("hi-1", "Kohinoor Devanagari", 12, 1700,
     "भारत एक विशाल देश है जहाँ अनेक भाषाएँ बोली जाती हैं। हर राज्य की अपनी संस्कृति, खान-पान और "
     "त्योहार हैं। लोग मिल-जुलकर रहते हैं और एक-दूसरे के उत्सवों में शामिल होते हैं।"),
    ("hi-2", "Devanagari Sangam MN", 12, 1700,
     "सुबह की ठंडी हवा में टहलना स्वास्थ्य के लिए बहुत लाभदायक है। नियमित व्यायाम से शरीर मजबूत "
     "रहता है और मन प्रसन्न रहता है। डॉक्टर हर व्यक्ति को प्रतिदिन कम से कम तीस मिनट चलने की "
     "सलाह देते हैं।"),
    ("hi-3", "Kohinoor Devanagari", 11, 1700,
     "विद्यालय में आज वार्षिक परीक्षा का परिणाम घोषित किया गया। सभी विद्यार्थियों ने कड़ी मेहनत की "
     "थी और अच्छे अंक प्राप्त किए। शिक्षकों ने बच्चों को बधाई दी और आगे भी परिश्रम करने के लिए "
     "प्रेरित किया।"),
    ("hien-1", "Kohinoor Devanagari", 12, 1700,
     "आज मैंने अपने नए laptop पर Python सीखना शुरू किया। इंटरनेट की speed बहुत अच्छी है और मैं रोज "
     "दो घंटे practice करता हूँ।"),
    ("bn-1", "Kohinoor Bangla", 12, 1700,
     "বাংলাদেশ ও পশ্চিমবঙ্গে বাংলা ভাষায় কথা বলা হয়। এই ভাষার একটি সমৃদ্ধ সাহিত্য ও সংস্কৃতি রয়েছে। "
     "রবীন্দ্রনাথ ঠাকুর এই ভাষায় অনেক কবিতা ও গান লিখেছেন।"),
    ("bn-2", "Bangla Sangam MN", 12, 1700,
     "সকালে নদীর তীরে হাঁটতে খুব ভালো লাগে। ঠান্ডা বাতাস আর পাখির ডাক মনকে শান্ত করে। প্রতিদিন "
     "কিছুক্ষণ হাঁটলে শরীর সুস্থ থাকে এবং কাজে মন বসে।"),
    ("bn-3", "Kohinoor Bangla", 11, 1700,
     "আজ স্কুলে বার্ষিক পরীক্ষার ফল প্রকাশিত হয়েছে। সব ছাত্রছাত্রী কঠোর পরিশ্রম করেছিল এবং ভালো "
     "নম্বর পেয়েছে। শিক্ষকেরা তাদের অভিনন্দন জানিয়েছেন।"),
]

spec_path, out_dir = sys.argv[1], sys.argv[2]
with open(spec_path, "w", encoding="utf-8") as f:
    for id_, font, size, width, text in SPEC:
        f.write(f"{id_}\t{font}\t{size}\t{width}\t{text}\n")
        with open(f"{out_dir}/{id_}.txt", "w", encoding="utf-8") as t:
            t.write(text + "\n")
