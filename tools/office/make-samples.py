# Makes the Office quality-test files with Microsoft Office itself (through COM), plus Office's own PDF of each
# (the reference). Output: testfiles/office/*.docx|xlsx|pptx and testfiles/office/ref/*.pdf.
# Run: E:/Anaconda/python.exe tools/office/make-samples.py   (needs Word, Excel and PowerPoint)
import os
import sys
import win32com.client as com

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(ROOT, "testfiles", "office")
REF = os.path.join(OUT, "ref")
PHOTO = os.path.join(ROOT, "testfiles", "photo.png")
os.makedirs(REF, exist_ok=True)

LOREM = (
    "Paginaut opens, edits and converts documents. This paragraph is here to fill lines the way ordinary prose does, "
    "with commas, numbers like 1,234.50 and 17%, a date such as 30 September 2026, and words of different lengths so "
    "that line breaks land in realistic places. "
)


def word_files(word):
    # 1. A report: headings, lists, tables (one crossing a page), a picture, footnote, header/footer, columns,
    #    a floating text box, Devanagari text, a landscape section.
    d = word.Documents.Add()
    s = word.Selection
    s.Style = d.Styles("Title")
    s.TypeText("Quarterly Report")
    s.TypeParagraph()
    s.Style = d.Styles("Subtitle")
    s.TypeText("Operations and finance, Q3 2026")
    s.TypeParagraph()
    s.Style = d.Styles("Heading 1")
    s.TypeText("1. Summary")
    s.TypeParagraph()
    s.Style = d.Styles("Normal")
    s.TypeText(LOREM * 2)
    s.Font.Bold = True
    s.TypeText("Revenue grew 12%")
    s.Font.Bold = False
    s.TypeText(" while costs were ")
    s.Font.Italic = True
    s.TypeText("flat")
    s.Font.Italic = False
    s.TypeText(". ")
    s.Font.Color = 0x0000C0
    s.TypeText("One region fell short.")
    s.Font.Color = 0
    d.Footnotes.Add(Range=s.Range, Text="Region figures are unaudited.")
    s.EndKey(6)
    s.TypeParagraph()
    s.Style = d.Styles("Heading 2")
    s.TypeText("1.1 Highlights")
    s.TypeParagraph()
    s.Style = d.Styles("List Bullet")
    for t in ["New office opened in Pune", "Two products launched", "Support response time halved"]:
        s.TypeText(t)
        s.TypeParagraph()
    s.Style = d.Styles("List Number")
    for t in ["Hire six engineers", "Close the audit", "Ship version 2"]:
        s.TypeText(t)
        s.TypeParagraph()
    s.Style = d.Styles("Normal")
    s.InlineShapes.AddPicture(PHOTO)
    s.TypeParagraph()
    s.Style = d.Styles("Caption")
    s.TypeText("Figure 1: The new office")
    s.TypeParagraph()
    s.Style = d.Styles("Heading 1")
    s.TypeText("2. Figures by region")
    s.TypeParagraph()
    s.Style = d.Styles("Normal")
    rows = 32
    t = d.Tables.Add(s.Range, rows + 1, 4)
    t.Style = "Grid Table 4 - Accent 1"
    for c, h in enumerate(["Region", "Revenue (₹ lakh)", "Cost (₹ lakh)", "Margin"], 1):
        t.Cell(1, c).Range.Text = h
    regions = ["North", "South", "East", "West", "Central", "Pune", "Mumbai", "Nagpur"]
    for r in range(rows):
        rev = 100 + r * 17 % 90
        cost = 60 + r * 11 % 50
        t.Cell(r + 2, 1).Range.Text = f"{regions[r % 8]} {r // 8 + 1}"
        t.Cell(r + 2, 2).Range.Text = f"{rev:,.2f}"
        t.Cell(r + 2, 3).Range.Text = f"{cost:,.2f}"
        t.Cell(r + 2, 4).Range.Text = f"{(rev - cost) / rev:.0%}"
    t.Rows(1).HeadingFormat = True
    s.EndKey(6)
    s.TypeParagraph()
    s.Style = d.Styles("Heading 1")
    s.TypeText("3. हिंदी और मराठी")
    s.TypeParagraph()
    s.Style = d.Styles("Normal")
    s.TypeText("यह अनुच्छेद हिंदी में है। पगिनॉट दस्तावेज़ खोलता और बदलता है। हा परिच्छेद मराठीत आहे, त्यात काही जोडाक्षरे आहेत: क्ष, ज्ञ, श्र.")
    s.TypeParagraph()
    # A floating text box next to the text.
    box = d.Shapes.AddTextbox(1, 380, 520, 150, 60)
    box.TextFrame.TextRange.Text = "Note: figures in lakh rupees."
    box.Fill.ForeColor.RGB = 0xE0F0FF
    # Two-column section.
    s.EndKey(6)
    s.InsertBreak(3)  # section break, continuous
    d.Sections(d.Sections.Count).PageSetup.TextColumns.SetCount(2)
    s.Style = d.Styles("Heading 1")
    s.TypeText("4. Notes in two columns")
    s.TypeParagraph()
    s.Style = d.Styles("Normal")
    for _ in range(3):
        s.TypeText(LOREM * 2)
        s.TypeParagraph()
    s.InsertBreak(2)  # section break, next page
    sec = d.Sections(d.Sections.Count)
    sec.PageSetup.TextColumns.SetCount(1)
    sec.PageSetup.Orientation = 1  # landscape
    s.Style = d.Styles("Heading 1")
    s.TypeText("5. Landscape appendix")
    s.TypeParagraph()
    s.Style = d.Styles("Normal")
    t2 = d.Tables.Add(s.Range, 4, 8)
    t2.Borders.Enable = True
    for r in range(1, 5):
        for c in range(1, 9):
            t2.Cell(r, c).Range.Text = f"R{r}C{c}"
    s.EndKey(6)
    s.TypeParagraph()
    s.TypeText("See ")
    d.Hyperlinks.Add(Anchor=s.Range, Address="https://mupdf.com", TextToDisplay="mupdf.com")
    s.EndKey(6)
    s.TypeText(" for the engine.")
    # Header and footer with a page number.
    for sec in d.Sections:
        sec.Headers(1).Range.Text = "Quarterly Report - Q3 2026"
        f = sec.Footers(1)
        f.PageNumbers.Add(1)
    save_word(d, "report")

    # 2. A letter: tabs, right-aligned date, justified text, signature block.
    d = word.Documents.Add()
    s = word.Selection
    s.Font.Name = "Calibri"
    s.ParagraphFormat.Alignment = 2
    s.TypeText("30 September 2026")
    s.TypeParagraph()
    s.ParagraphFormat.Alignment = 0
    for line in ["Ms Asha Kulkarni", "14 Tilak Road", "Pune 411002"]:
        s.TypeText(line)
        s.TypeParagraph()
    s.TypeParagraph()
    s.Font.Bold = True
    s.TypeText("Subject: Renewal of the service agreement")
    s.Font.Bold = False
    s.TypeParagraph()
    s.TypeText("Dear Ms Kulkarni,")
    s.TypeParagraph()
    s.ParagraphFormat.Alignment = 3
    for _ in range(4):
        s.TypeText(LOREM * 2)
        s.TypeParagraph()
    s.ParagraphFormat.Alignment = 0
    s.ParagraphFormat.TabStops.Add(250)
    s.TypeText("Item\tAmount")
    s.TypeParagraph()
    s.TypeText("Annual fee\t₹ 1,20,000")
    s.TypeParagraph()
    s.TypeText("GST 18%\t₹ 21,600")
    s.TypeParagraph()
    s.TypeParagraph()
    s.TypeText("Yours sincerely,")
    s.TypeParagraph()
    s.TypeParagraph()
    s.TypeText("Rahul Deshpande")
    s.TypeParagraph()
    s.Font.Italic = True
    s.TypeText("Head of Accounts")
    save_word(d, "letter")

    # Password-protected (no reference PDF): converters must fail cleanly, not wait for a password.
    d = word.Documents.Add()
    word.Selection.TypeText("Secret contents")
    d.SaveAs2(os.path.join(OUT, "locked.docx"), 16, False, "secret")
    d.Close(0)
    print("made locked.docx")


def save_word(d, name):
    d.SaveAs2(os.path.join(OUT, name + ".docx"), 16)
    d.ExportAsFixedFormat(os.path.join(REF, name + ".pdf"), 17)
    d.Close(0)
    print("made", name + ".docx")


def excel_files(xl):
    # A budget: title merged across columns, formats (currency, percent, dates), formulas, borders, fills,
    # a chart, and a second sheet too wide for one page.
    wb = xl.Workbooks.Add()
    ws = wb.Worksheets(1)
    ws.Name = "Budget"
    ws.Range("A1:F1").Merge()
    ws.Range("A1").Value = "Household budget 2026"
    ws.Range("A1").Font.Size = 16
    ws.Range("A1").Font.Bold = True
    ws.Range("A1").HorizontalAlignment = -4108
    heads = ["Month", "Income", "Rent", "Food", "Savings", "Saved %"]
    for c, h in enumerate(heads, 1):
        cell = ws.Cells(3, c)
        cell.Value = h
        cell.Font.Bold = True
        cell.Interior.Color = 0xF2D9C6
    for m in range(12):
        r = 4 + m
        ws.Cells(r, 1).Value = f"2026-{m + 1:02d}-01"
        ws.Cells(r, 1).NumberFormat = "mmm yyyy"
        ws.Cells(r, 2).Value = 85000 + m * 1500
        ws.Cells(r, 3).Value = 22000
        ws.Cells(r, 4).Value = 12000 + (m * 731) % 4000
        ws.Cells(r, 5).Formula = f"=B{r}-C{r}-D{r}"
        ws.Cells(r, 6).Formula = f"=E{r}/B{r}"
    ws.Range("B4:E16").NumberFormat = "₹ #,##,##0"
    ws.Range("F4:F16").NumberFormat = "0.0%"
    ws.Cells(16, 1).Value = "Total"
    ws.Cells(16, 1).Font.Bold = True
    for c in "BCDE":
        ws.Range(f"{c}16").Formula = f"=SUM({c}4:{c}15)"
    ws.Range("F16").Formula = "=E16/B16"
    ws.Range("A3:F16").Borders.LineStyle = 1
    ws.Range("A16:F16").Font.Bold = True
    ws.Columns("A:F").AutoFit()
    ch = ws.ChartObjects().Add(20, 280, 420, 220)
    ch.Chart.SetSourceData(ws.Range("A3:A15,E3:E15"))
    ch.Chart.ChartType = 51  # clustered column
    ch.Chart.HasTitle = True
    ch.Chart.ChartTitle.Text = "Saved each month"
    ws2 = wb.Worksheets.Add(After=ws)
    ws2.Name = "Wide"
    for c in range(1, 21):
        ws2.Cells(1, c).Value = f"Column {c}"
        ws2.Cells(1, c).Font.Bold = True
        for r in range(2, 60):
            ws2.Cells(r, c).Value = (r * 37 + c * 11) % 1000 / 10
    ws2.Range("A1:T59").NumberFormat = "0.0"
    ws2.Columns("A:T").ColumnWidth = 12
    save_excel(wb, "budget")

    # An invoice fitted to one page, with wrapped text and a logo.
    wb = xl.Workbooks.Add()
    ws = wb.Worksheets(1)
    ws.Name = "Invoice"
    ws.Shapes.AddPicture(PHOTO, 0, 1, 360, 5, 80, 60)
    ws.Range("A1").Value = "INVOICE"
    ws.Range("A1").Font.Size = 22
    ws.Range("A1").Font.Bold = True
    ws.Range("A3").Value = "Bill to: Kulkarni Traders, 14 Tilak Road, Pune 411002, Maharashtra, India"
    ws.Range("A3:D3").Merge()
    ws.Range("A3").WrapText = True
    ws.Rows(3).RowHeight = 32
    for c, h in enumerate(["Item", "Qty", "Rate", "Amount"], 1):
        ws.Cells(5, c).Value = h
        ws.Cells(5, c).Font.Bold = True
        ws.Cells(5, c).Interior.Color = 0x404040
        ws.Cells(5, c).Font.Color = 0xFFFFFF
    items = ["Consulting (hours)", "Travel", "Licence, annual", "Training day", "Printed manuals, bound"]
    for i, it in enumerate(items):
        r = 6 + i
        ws.Cells(r, 1).Value = it
        ws.Cells(r, 2).Value = i + 2
        ws.Cells(r, 3).Value = 1500 * (i + 1)
        ws.Cells(r, 4).Formula = f"=B{r}*C{r}"
    ws.Cells(12, 3).Value = "Total"
    ws.Cells(12, 3).Font.Bold = True
    ws.Cells(12, 4).Formula = "=SUM(D6:D10)"
    ws.Range("C6:D12").NumberFormat = "₹ #,##,##0.00"
    ws.Columns("A").ColumnWidth = 30
    ws.Columns("B:D").ColumnWidth = 14
    ws.Range("A5:D12").Borders(9).LineStyle = 1
    ws.PageSetup.Zoom = False
    ws.PageSetup.FitToPagesWide = 1
    ws.PageSetup.FitToPagesTall = 1
    save_excel(wb, "invoice")

    wb = xl.Workbooks.Add()
    wb.Worksheets(1).Range("A1").Value = "Secret"
    wb.SaveAs(os.path.join(OUT, "locked.xlsx"), 51, "secret")
    wb.Close(False)
    print("made locked.xlsx")


def save_excel(wb, name):
    wb.SaveAs(os.path.join(OUT, name + ".xlsx"), 51)
    wb.ExportAsFixedFormat(0, os.path.join(REF, name + ".pdf"))
    wb.Close(False)
    print("made", name + ".xlsx")


def powerpoint_files(pp):
    p = pp.Presentations.Add(False)
    lay = lambda i: p.SlideMaster.CustomLayouts(i)
    s = p.Slides.AddSlide(1, lay(1))
    s.Shapes(1).TextFrame.TextRange.Text = "Paginaut 2026"
    s.Shapes(2).TextFrame.TextRange.Text = "Plans for the year"
    s = p.Slides.AddSlide(2, lay(2))
    s.Shapes(1).TextFrame.TextRange.Text = "What we'll do"
    s.Shapes(2).TextFrame.TextRange.Text = "Open Office files\rConvert them to PDF\r\tWith the best engine found\rCompare versions"
    s = p.Slides.AddSlide(3, lay(6))
    s.Shapes(1).TextFrame.TextRange.Text = "The new office"
    s.Shapes.AddPicture(PHOTO, False, True, 100, 120, 400, 300)
    s = p.Slides.AddSlide(4, lay(6))
    s.Shapes(1).TextFrame.TextRange.Text = "Numbers"
    tb = s.Shapes.AddTable(4, 3, 60, 130, 600, 200).Table
    for r, row in enumerate([["Quarter", "Users", "Growth"], ["Q1", "1,200", "—"], ["Q2", "1,950", "63%"], ["Q3", "3,100", "59%"]], 1):
        for c, v in enumerate(row, 1):
            tb.Cell(r, c).Shape.TextFrame.TextRange.Text = v
    s = p.Slides.AddSlide(5, lay(6))
    s.Shapes(1).TextFrame.TextRange.Text = "Shapes"
    for i, kind in enumerate([1, 9, 5, 33]):  # rectangle, oval, rounded rectangle, arrow
        sh = s.Shapes.AddShape(kind, 60 + i * 160, 180, 130, 100)
        sh.TextFrame.TextRange.Text = ["Plan", "Build", "Test", "Ship"][i]
    s = p.Slides.AddSlide(6, lay(6))
    s.Shapes(1).TextFrame.TextRange.Text = "Growth"
    try:
        s.Shapes.AddChart2(-1, 51, 60, 120, 600, 340)
    except Exception:  # charts need PowerPoint's window (Excel embedding); fall back to a drawn one
        for i, v in enumerate([120, 195, 310]):
            bar = s.Shapes.AddShape(1, 120 + i * 180, 440 - v, 110, v)
            bar.TextFrame.TextRange.Text = ["Q1", "Q2", "Q3"][i]
    s = p.Slides.AddSlide(7, lay(2))
    s.Shapes(1).TextFrame.TextRange.Text = "हिंदी और मराठी"
    s.Shapes(2).TextFrame.TextRange.Text = "नमस्ते दुनिया\rमराठी भाषा\rक्ष, ज्ञ, श्र"
    p.SaveAs(os.path.join(OUT, "deck.pptx"), 24)
    p.SaveAs(os.path.join(REF, "deck.pdf"), 32)
    p.Close()
    print("made deck.pptx")

    p = pp.Presentations.Add(False)
    s = p.Slides.AddSlide(1, p.SlideMaster.CustomLayouts(1))
    s.Shapes(1).TextFrame.TextRange.Text = "Secret"
    p.Password = "secret"
    p.SaveAs(os.path.join(OUT, "locked.pptx"), 24)
    p.Close()
    print("made locked.pptx")


def main():
    which = sys.argv[1:] or ["word", "excel", "powerpoint"]
    # Samples only: an existing file is replaced.
    if "word" in which:
        word = com.DispatchEx("Word.Application")
        word.Visible = False
        word.DisplayAlerts = 0
        try:
            word_files(word)
        finally:
            word.Quit(0)
    if "excel" in which:
        xl = com.DispatchEx("Excel.Application")
        xl.Visible = False
        xl.DisplayAlerts = False
        try:
            excel_files(xl)
        finally:
            xl.Quit()
    if "powerpoint" in which:
        pp = com.DispatchEx("PowerPoint.Application")
        try:
            powerpoint_files(pp)
        finally:
            pp.Quit()


main()
