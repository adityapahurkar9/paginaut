# Makes small PDFs for examples/pdfa_test.rs (testfiles isn't in git): python tools/make-pdfa-samples.py testfiles
#   rights.pdf        XMP with standard rights properties and a document ID: Save as PDF/A keeps them
#   labelled.pdf      XMP with a labelling tool's own properties (not PDF/A): they're dropped
#   untyped-font.pdf  a font dictionary without /Type (some producers leave it out): still found and embedded
import os, sys

def make(path, xmp=None, font=b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"):
    content = b"BT /F1 24 Tf 72 700 Td (PDF/A sample) Tj ET\n"
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R" + (b" /Metadata 6 0 R" if xmp else b"") + b" >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
        font,
        b"<< /Type /Metadata /Subtype /XML /Length %d >>\nstream\n" % len(xmp.encode()) + xmp.encode() + b"\nendstream" if xmp else b"null",
        b"<< /Title (Rights test) /Author (Paginaut) >>",
    ]
    pdf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    offs = []
    for i, o in enumerate(objs, 1):
        offs.append(len(pdf))
        pdf += b"%d 0 obj\n" % i + o + b"\nendobj\n"
    xref = len(pdf)
    pdf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for o in offs:
        pdf += b"%010d 00000 n \n" % o
    pdf += b"trailer\n<< /Size %d /Root 1 0 R /Info 7 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
    open(path, "wb").write(pdf)

head = '<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
tail = '</rdf:RDF></x:xmpmeta><?xpacket end="w"?>'
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
make(os.path.join(out, "rights.pdf"), head +
     '<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/"'
     ' xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/" xmpRights:Marked="True" xmpMM:DocumentID="uuid:1234-abcd">'
     '<dc:title><rdf:Alt><rdf:li xml:lang="x-default">Rights test</rdf:li></rdf:Alt></dc:title>'
     '<dc:rights><rdf:Alt><rdf:li xml:lang="x-default">Copyright 2026 Paginaut tests</rdf:li></rdf:Alt></dc:rights>'
     '<dc:subject><rdf:Bag><rdf:li>archive</rdf:li><rdf:li>test</rdf:li></rdf:Bag></dc:subject>'
     '</rdf:Description>' + tail)
make(os.path.join(out, "labelled.pdf"), head +
     '<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:pdfx="http://ns.adobe.com/pdfx/1.3/"'
     ' pdfx:MSIP_Label_Enabled="true">'
     '<dc:rights><rdf:Alt><rdf:li xml:lang="x-default">Dropped with the unknown schema around it</rdf:li></rdf:Alt></dc:rights>'
     '</rdf:Description>' + tail)
make(os.path.join(out, "untyped-font.pdf"), font=b"<< /Subtype /TrueType /BaseFont /Arial /Encoding /WinAnsiEncoding >>")
print("ok")
