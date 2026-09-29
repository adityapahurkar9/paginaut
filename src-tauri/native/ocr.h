/* Tesseract bridge (ocr.cpp), used by paginaut.c. */
#ifndef PAGINAUT_OCR_H
#define PAGINAUT_OCR_H

#ifdef __cplusplus
extern "C" {
#endif

#ifndef PO_TESS_DEFINED
#define PO_TESS_DEFINED
typedef struct po_tess po_tess;
#endif

/* A recogniser for langs ("eng", "eng+hin"). files lists the model for each
 * language as "lang\tC:\\path\\lang.traineddata\n" lines (UTF-8). Keep it on
 * one thread. On failure returns NULL with a message in err. */
po_tess *po_tess_new(const char *langs, const char *files, char *err, int errlen);
void po_tess_drop(po_tess *t);

/* One recognised word, in reading order, in image pixels: the box of its line,
 * the line's baseline (x1,y1,x2,y2), the word's box, confidence 0-100, UTF-8. */
typedef void (*po_word_fn)(void *arg, int line, const int line_box[4], const int baseline[4],
	const int box[4], float conf, const char *text);

/* Recognises an 8-bit grey image. Returns 0, 1 if *abort_flag became non-zero, -1 on error. */
int po_tess_recognize(po_tess *t, const unsigned char *gray, int w, int h, int stride, int dpi,
	po_word_fn word, void *arg, volatile int *abort_flag);

#ifdef __cplusplus
}
#endif

#endif
