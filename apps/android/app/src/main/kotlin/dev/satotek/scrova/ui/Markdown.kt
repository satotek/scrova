package dev.satotek.scrova.ui

internal data class MarkdownBlock(val text: String, val heading: Int = 0, val bullet: Boolean = false)

// The bundled policy uses only paragraphs, headings and bullets. Keeping this
// subset local avoids a WebView or a Markdown runtime for an offline document.
internal fun markdownBlocks(source: String): List<MarkdownBlock> {
    val result = mutableListOf<MarkdownBlock>()
    var paragraph = StringBuilder()
    var bullet = false
    fun flush() {
        if (paragraph.isNotEmpty()) result += MarkdownBlock(paragraph.toString(), bullet = bullet)
        paragraph = StringBuilder()
        bullet = false
    }
    source.lines().forEach { line ->
        val text = line.trim()
        when {
            text.isEmpty() -> flush()
            text.startsWith("#") -> {
                flush()
                val depth = text.takeWhile { it == '#' }.length
                result += MarkdownBlock(text.drop(depth).trim(), heading = depth)
            }
            text.startsWith("- ") -> {
                flush()
                bullet = true
                paragraph.append(text.drop(2))
            }
            else -> {
                if (paragraph.isNotEmpty()) paragraph.append(' ')
                paragraph.append(text)
            }
        }
    }
    flush()
    return result
}
