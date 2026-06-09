/*
 * cjk_tokenizer.c
 *
 * SQLite FTS5 custom tokenizer with CJK bigram support.
 *
 * For CJK codepoints (Chinese, Japanese, Korean), generates overlapping
 * bigrams so that any 2-character substring can be matched.
 * For ASCII / Latin text, falls back to whitespace + punctuation splitting.
 *
 * Build:
 *   gcc -O2 -fPIC -shared -o libcjkfts.so cjk_tokenizer.c \
 *       $(pkg-config --cflags sqlite3)
 *
 * Usage in SQLite:
 *   SELECT load_extension('./libcjkfts');
 *   CREATE VIRTUAL TABLE t USING fts5(body, tokenize='cjk_bigram');
 *   INSERT INTO t VALUES ('今日の天気はいいです');
 *   SELECT * FROM t WHERE t MATCH '天気';
 */

#include <sqlite3ext.h>
SQLITE_EXTENSION_INIT1

#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>

/* -------------------------------------------------------------------------
 * Unicode helpers
 * ---------------------------------------------------------------------- */

/*
 * Decode one UTF-8 sequence starting at *p (which must be < end).
 * Writes the codepoint to *cp and advances *p past the sequence.
 * Returns the byte-length of the sequence (1-4), or 0 on error.
 */
static int utf8_decode(const unsigned char *p, const unsigned char *end,
                       uint32_t *cp)
{
    unsigned char c = *p;
    int len;
    uint32_t codepoint;

    if (c < 0x80) {
        *cp = c;
        return 1;
    } else if ((c & 0xE0) == 0xC0) {
        len = 2; codepoint = c & 0x1F;
    } else if ((c & 0xF0) == 0xE0) {
        len = 3; codepoint = c & 0x0F;
    } else if ((c & 0xF8) == 0xF0) {
        len = 4; codepoint = c & 0x07;
    } else {
        *cp = 0xFFFD;   /* replacement character for bad bytes */
        return 1;
    }

    if (p + len > end) {
        *cp = 0xFFFD;
        return (int)(end - p);
    }

    for (int i = 1; i < len; i++) {
        if ((p[i] & 0xC0) != 0x80) {
            *cp = 0xFFFD;
            return i;
        }
        codepoint = (codepoint << 6) | (p[i] & 0x3F);
    }

    *cp = codepoint;
    return len;
}

/*
 * Returns 1 if the codepoint belongs to a CJK range that should be
 * tokenised via bigram.  Covers the major blocks used in Chinese,
 * Japanese, and Korean text.
 */
static int is_cjk(uint32_t cp)
{
    return (
        /* CJK Unified Ideographs (most common Han characters) */
        (cp >= 0x4E00  && cp <= 0x9FFF)  ||
        /* CJK Unified Ideographs Extension A */
        (cp >= 0x3400  && cp <= 0x4DBF)  ||
        /* CJK Unified Ideographs Extension B */
        (cp >= 0x20000 && cp <= 0x2A6DF) ||
        /* CJK Compatibility Ideographs */
        (cp >= 0xF900  && cp <= 0xFAFF)  ||
        /* Hiragana */
        (cp >= 0x3040  && cp <= 0x309F)  ||
        /* Katakana */
        (cp >= 0x30A0  && cp <= 0x30FF)  ||
        /* Hangul Syllables (Korean) */
        (cp >= 0xAC00  && cp <= 0xD7AF)  ||
        /* Hangul Jamo */
        (cp >= 0x1100  && cp <= 0x11FF)  ||
        /* CJK Symbols and Punctuation (e.g. 。、） */
        (cp >= 0x3000  && cp <= 0x303F)  ||
        /* Halfwidth / Fullwidth Forms */
        (cp >= 0xFF00  && cp <= 0xFFEF)
    );
}

/* Returns 1 for codepoints that should be treated as token delimiters */
static int is_separator(uint32_t cp)
{
    /* ASCII whitespace / punctuation */
    if (cp < 0x80) {
        return !isalnum((int)cp);
    }
    /* CJK punctuation already counted inside is_cjk; skip standalone */
    return 0;
}

/* -------------------------------------------------------------------------
 * Token buffer
 * ---------------------------------------------------------------------- */

/*
 * We keep a small ring-buffer of the last 2 CJK codepoints so we can emit
 * bigrams without re-scanning.  For each CJK char we store its byte-offset
 * and byte-length in the original string.
 */
typedef struct {
    int start;   /* byte offset of this char in the source string */
    int len;     /* byte length of this char's UTF-8 encoding */
} CharPos;

/* -------------------------------------------------------------------------
 * ASCII case-folding helpers
 * ---------------------------------------------------------------------- */

static void ascii_lowercase(const char *src, int len, char *dst)
{
    for (int i = 0; i < len; i++) {
        unsigned char c = (unsigned char)src[i];
        dst[i] = (c >= 'A' && c <= 'Z') ? (char)(c + 32) : (char)c;
    }
    dst[len] = '\0';
}

/* -------------------------------------------------------------------------
 * FTS5 tokenizer callbacks
 * ---------------------------------------------------------------------- */

typedef struct CjkTokenizer CjkTokenizer;
struct CjkTokenizer {
    /* nothing configurable yet */
    int dummy;
};

static int cjk_create(void *pCtx, const char **azArg, int nArg,
                      Fts5Tokenizer **ppOut)
{
    (void)pCtx; (void)azArg; (void)nArg;
    CjkTokenizer *p = (CjkTokenizer *)sqlite3_malloc(sizeof(CjkTokenizer));
    if (!p) return SQLITE_NOMEM;
    p->dummy = 0;
    *ppOut = (Fts5Tokenizer *)p;
    return SQLITE_OK;
}

static void cjk_delete(Fts5Tokenizer *pTok)
{
    sqlite3_free(pTok);
}

/*
 * Main tokenize function.
 *
 * Strategy:
 *   - Walk the input byte-by-byte decoding UTF-8.
 *   - Maintain a small window of the last 2 CJK character positions.
 *   - When we hit a CJK character:
 *       * Flush any pending ASCII token first.
 *       * Emit a single-character token for the current char.
 *       * If we have a previous CJK char, also emit a bigram token
 *         spanning (prev, current).
 *   - For ASCII letters/digits: accumulate them; emit on separator.
 *   - Ignore separators.
 */
static int cjk_tokenize(Fts5Tokenizer *pTok, void *pCtx, int flags,
                         const char *pText, int nText,
                         int (*xToken)(void*, int, const char*, int, int, int))
{
    (void)pTok;
    (void)flags;

    if (!pText) return SQLITE_OK;
    if (nText < 0) nText = (int)strlen(pText);

    const unsigned char *src  = (const unsigned char *)pText;
    const unsigned char *end  = src + nText;
    const unsigned char *cur  = src;

    /* State for pending ASCII run */
    int ascii_start = -1;   /* byte offset of current ASCII token start */

    /* Ring buffer for last 2 CJK positions */
    CharPos cjk_ring[2];
    int     cjk_count = 0;  /* how many CJK chars accumulated so far */

    int rc = SQLITE_OK;

    /* Scratch buffer for ASCII case-folding (stack-allocated, grown as needed) */
    char  ascii_buf_small[64];
    char *ascii_buf      = ascii_buf_small;
    int   ascii_buf_cap  = 64;

    /*
     * Helper: flush the accumulated ASCII token ending just before the
     * current character (char_start), then reset ascii_start.
     * 'end_offset' is passed explicitly so callers can control where the
     * token ends — usually char_start (before the current non-ASCII char).
     */
#define FLUSH_ASCII_AT(end_offset) do { \
    if (ascii_start >= 0) { \
        int tok_start = ascii_start; \
        int tok_end   = (end_offset); \
        int tok_len   = tok_end - tok_start; \
        ascii_start   = -1; \
        if (tok_len > 0) { \
            if (tok_len + 1 > ascii_buf_cap) { \
                if (ascii_buf != ascii_buf_small) sqlite3_free(ascii_buf); \
                ascii_buf = (char *)sqlite3_malloc(tok_len + 1); \
                if (!ascii_buf) { rc = SQLITE_NOMEM; goto done; } \
                ascii_buf_cap = tok_len + 1; \
            } \
            ascii_lowercase(pText + tok_start, tok_len, ascii_buf); \
            rc = xToken(pCtx, 0, ascii_buf, tok_len, tok_start, tok_end); \
            if (rc != SQLITE_OK) goto done; \
        } \
    } \
    cjk_count = 0; \
} while(0)

    while (cur < end && rc == SQLITE_OK) {
        uint32_t cp;
        int char_start = (int)(cur - src);
        int char_len   = utf8_decode(cur, end, &cp);
        cur += char_len;

        if (cp == 0xFFFD && char_len == 0) break; /* safety */

        if (is_cjk(cp)) {
            /* Flush any pending ASCII ending before this CJK char */
            FLUSH_ASCII_AT(char_start);

            CharPos me = { char_start, char_len };

            /* Emit single-char token */
            rc = xToken(pCtx, 0,
                        pText + me.start, me.len,
                        me.start, me.start + me.len);
            if (rc != SQLITE_OK) goto done;

            /* Emit bigram with the previous CJK char */
            if (cjk_count >= 1) {
                CharPos prev = cjk_ring[(cjk_count - 1) % 2];
                int bi_start = prev.start;
                int bi_end   = me.start + me.len;
                rc = xToken(pCtx, 0,
                            pText + bi_start, bi_end - bi_start,
                            bi_start, bi_end);
                if (rc != SQLITE_OK) goto done;
            }

            /* Save into ring buffer */
            cjk_ring[cjk_count % 2] = me;
            cjk_count++;

        } else if (!is_separator(cp)) {
            /* ASCII / Latin letter or digit — accumulate */
            cjk_count = 0;   /* break CJK bigram chain */
            if (ascii_start < 0) {
                ascii_start = char_start;
            }
        } else {
            /* Separator: flush ASCII, break CJK chain */
            FLUSH_ASCII_AT(char_start);
        }
    }

    /* Flush any trailing ASCII token */
    if (ascii_start >= 0) {
        int tok_end = (int)(cur - src);
        int tok_len = tok_end - ascii_start;
        if (tok_len + 1 > ascii_buf_cap) {
            if (ascii_buf != ascii_buf_small) sqlite3_free(ascii_buf);
            ascii_buf = (char *)sqlite3_malloc(tok_len + 1);
            if (!ascii_buf) { rc = SQLITE_NOMEM; goto done; }
        }
        ascii_lowercase(pText + ascii_start, tok_len, ascii_buf);
        rc = xToken(pCtx, 0, ascii_buf, tok_len, ascii_start, tok_end);
    }

done:
    if (ascii_buf != ascii_buf_small) sqlite3_free(ascii_buf);
#undef FLUSH_ASCII_AT
    return rc;
}

/* -------------------------------------------------------------------------
 * Extension entry point
 * ---------------------------------------------------------------------- */

#ifdef _WIN32
__declspec(dllexport)
#endif
int sqlite3_cjkfts_init(sqlite3 *db, char **pzErrMsg,
                         const sqlite3_api_routines *pApi)
{
    SQLITE_EXTENSION_INIT2(pApi);
    (void)pzErrMsg;

    /* FTS5 tokenizer API is retrieved via fts5_api */
    fts5_api *pFts5 = NULL;

    /* Standard way to get fts5_api pointer from SQLite ≥ 3.20 */
    sqlite3_stmt *pStmt = NULL;
    int rc = sqlite3_prepare(db, "SELECT fts5(?1)", -1, &pStmt, NULL);
    if (rc == SQLITE_OK) {
        sqlite3_bind_pointer(pStmt, 1, (void*)&pFts5, "fts5_api_ptr", NULL);
        sqlite3_step(pStmt);
        sqlite3_finalize(pStmt);
    }

    if (!pFts5) {
        if (pzErrMsg)
            *pzErrMsg = sqlite3_mprintf("cannot get fts5_api pointer");
        return SQLITE_ERROR;
    }

    /* Register our tokenizer */
    fts5_tokenizer tok = {
        cjk_create,
        cjk_delete,
        cjk_tokenize
    };

    rc = pFts5->xCreateTokenizer(pFts5, "cjk_bigram", (void*)pFts5, &tok, NULL);
    return rc;
}
