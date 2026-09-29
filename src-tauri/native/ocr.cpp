/* Paginaut's bridge to Tesseract (the copy built into MuPDF's libraries).
 *
 * MuPDF's own OCR entry points load the language models again for every page
 * and allow only one recognition at a time in the whole process. Here each
 * worker thread keeps its own recogniser, loaded once per job, so pages are
 * recognised in parallel.
 *
 * Models are read through a file reader of ours: each "<lang>.traineddata"
 * Tesseract asks for is looked up by name in a table of real paths (UTF-8,
 * possibly non-ASCII, which Tesseract's own narrow-character reader can't open).
 */
#include "ocr.h"

#include <tesseract/baseapi.h>
#include <tesseract/ocrclass.h>
#include <tesseract/resultiterator.h>

#include <mutex>
#include <string>
#include <vector>
#include <cstdio>
#include <cstring>

#ifdef _WIN32
#include <windows.h>
#endif

namespace {

/* Language → model file, for the recogniser being initialised (under g_init). */
std::mutex g_init;
std::vector<std::pair<std::string, std::string>> g_files;

FILE *open_utf8(const std::string &path)
{
#ifdef _WIN32
	int n = MultiByteToWideChar(CP_UTF8, 0, path.c_str(), -1, nullptr, 0);
	if (n <= 0)
		return nullptr;
	std::wstring w((size_t)n, L'\0');
	MultiByteToWideChar(CP_UTF8, 0, path.c_str(), -1, &w[0], n);
	return _wfopen(w.c_str(), L"rb");
#else
	return fopen(path.c_str(), "rb");
#endif
}

bool read_model(const char *filename, std::vector<char> *out)
{
	std::string name(filename);
	size_t slash = name.find_last_of("/\\");
	if (slash != std::string::npos)
		name = name.substr(slash + 1);
	const std::string ext = ".traineddata";
	if (name.size() > ext.size() && name.compare(name.size() - ext.size(), ext.size(), ext) == 0)
		name.resize(name.size() - ext.size());
	for (auto &f : g_files)
	{
		if (f.first != name)
			continue;
		FILE *fp = open_utf8(f.second);
		if (!fp)
			return false;
		bool ok = false;
		if (fseek(fp, 0, SEEK_END) == 0)
		{
			long size = ftell(fp);
			fseek(fp, 0, SEEK_SET);
			if (size > 0)
			{
				out->resize((size_t)size);
				ok = fread(out->data(), 1, (size_t)size, fp) == (size_t)size;
			}
		}
		fclose(fp);
		return ok;
	}
	return false;
}

bool cancelled(void *flag, int)
{
	return flag && *(volatile int *)flag != 0;
}

} // namespace

struct po_tess
{
	tesseract::TessBaseAPI api;
};

po_tess *po_tess_new(const char *langs, const char *files, char *err, int errlen)
{
	po_tess *t = new (std::nothrow) po_tess;
	if (!t)
	{
		snprintf(err, (size_t)errlen, "out of memory");
		return nullptr;
	}
	int rc;
	{
		/* Tesseract's parameters are partly global; load one recogniser at a time. */
		std::lock_guard<std::mutex> lock(g_init);
		g_files.clear();
		for (const char *p = files; p && *p;)
		{
			const char *tab = strchr(p, '\t');
			const char *nl = strchr(p, '\n');
			if (!nl)
				nl = p + strlen(p);
			if (tab && tab < nl)
				g_files.emplace_back(std::string(p, tab), std::string(tab + 1, nl));
			p = *nl ? nl + 1 : nl;
		}
		rc = t->api.Init(nullptr, 0, langs, tesseract::OEM_LSTM_ONLY, nullptr, 0, nullptr, nullptr, false, &read_model);
		g_files.clear();
	}
	if (rc != 0)
	{
		snprintf(err, (size_t)errlen, "couldn't load the language models (%s)", langs);
		delete t;
		return nullptr;
	}
	t->api.SetPageSegMode(tesseract::PSM_AUTO);
	return t;
}

void po_tess_drop(po_tess *t)
{
	if (!t)
		return;
	t->api.End();
	delete t;
}

int po_tess_recognize(po_tess *t, const unsigned char *gray, int w, int h, int stride, int dpi,
	po_word_fn word, void *arg, volatile int *abort_flag)
{
	t->api.SetImage(gray, w, h, 1, stride);
	t->api.SetSourceResolution(dpi);
	tesseract::ETEXT_DESC monitor;
	monitor.cancel = &cancelled;
	monitor.cancel_this = (void *)abort_flag;
	int rc = t->api.Recognize(&monitor);
	if (abort_flag && *abort_flag)
	{
		t->api.Clear();
		return 1;
	}
	if (rc < 0)
	{
		t->api.Clear();
		return -1;
	}
	tesseract::ResultIterator *it = t->api.GetIterator();
	int line = -1;
	int line_box[4] = { 0 }, base[4] = { 0 };
	if (it)
	{
		do
		{
			if (it->Empty(tesseract::RIL_WORD))
				continue;
			if (line < 0 || it->IsAtBeginningOf(tesseract::RIL_TEXTLINE))
			{
				line++;
				it->BoundingBox(tesseract::RIL_TEXTLINE, &line_box[0], &line_box[1], &line_box[2], &line_box[3]);
				if (!it->Baseline(tesseract::RIL_TEXTLINE, &base[0], &base[1], &base[2], &base[3]))
				{
					base[0] = line_box[0];
					base[1] = base[3] = line_box[3];
					base[2] = line_box[2];
				}
			}
			int box[4];
			if (!it->BoundingBox(tesseract::RIL_WORD, &box[0], &box[1], &box[2], &box[3]))
				continue;
			char *text = it->GetUTF8Text(tesseract::RIL_WORD);
			if (text && *text)
				word(arg, line, line_box, base, box, it->Confidence(tesseract::RIL_WORD), text);
			delete[] text;
		} while (it->Next(tesseract::RIL_WORD));
		delete it;
	}
	t->api.Clear();
	return 0;
}
