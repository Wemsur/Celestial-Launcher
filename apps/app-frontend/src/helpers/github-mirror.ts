// GitHub 加速镜像。
//
// 启动器自己的商店索引、以及商店条目指向的下载地址，都托管在 GitHub 上。
// 直连 `raw.githubusercontent.com` 在部分网络下会整段超时，`github.com/.../releases/download/...`
// 尤其严重（实测 60 秒零字节），所以这里给商店提供一层可选的镜像前缀。
//
// 只重写已知的 GitHub 主机：镜像站点是「把原始 URL 拼在自己后面」的代理，
// 把任意 URL 拼过去既没有意义，也可能被镜像拿去当开放代理使，所以先判主机再拼。
//
// 镜像可用性实测（2026-10-16 ~ 10-17，见会话记录）：
//   - `github.tooldelta.top`  最快：索引 0.23s、资产 0.49s，sha256 与直连一致，
//                             releases/latest 302 可用，404 是真 404，
//                             保留 Authorization 头（假 token 得 401），但不代理 api.github.com（403）
//   - `ghfast.top`            索引 1.5s，资产下载 3.8s，releases/latest 302 可用
//   - `gh-proxy.com`          索引 7.4s，资产下载 2.7s，api.github.com 唯一可用
//                             （但会丢弃 auth 头，改用它自己的账号，绝不能带 token 用）
//   - `ghproxy.net`           索引 2.6s，资产下载不稳定（一次 10.7s 超时）
// 已被排除的：gh.con.sh（HTTP 200 但正文只有 48 字节的 "Suspent due to abuse report."，
// 是危险的假 200）、ghproxy.cc（TLS 证书过期）、gitmirror/gitdl/ghp.ci（DNS 解析失败）、
// gitproxy.click（401）、raw.githack.com（超时）。
//
// 两个商店只取索引，索引都在 raw.githubusercontent.com，四个镜像都可用；
// 所以这里不做按端点分层，只让用户挑一个。

/** 一个镜像站。`prefix` 是原始 URL 要拼在其后的前缀。 */
export interface GithubMirror {
	/** 存进偏好文件的稳定标识，也是域名本身。 */
	id: string
	label: string
	prefix: string
}

export const GITHUB_MIRRORS: GithubMirror[] = [
	{
		id: 'github.tooldelta.top',
		label: 'github.tooldelta.top',
		prefix: 'https://github.tooldelta.top/',
	},
	{ id: 'ghfast.top', label: 'ghfast.top', prefix: 'https://ghfast.top/' },
	{ id: 'gh-proxy.com', label: 'gh-proxy.com', prefix: 'https://gh-proxy.com/' },
	{ id: 'ghproxy.net', label: 'ghproxy.net', prefix: 'https://ghproxy.net/' },
]

export const DEFAULT_MIRROR_ID = 'github.tooldelta.top'

/** 允许被重写的 GitHub 主机。其它主机一律原样返回。 */
const MIRRORABLE_HOSTS = [
	'github.com',
	'raw.githubusercontent.com',
	'api.github.com',
	'objects.githubusercontent.com',
	'release-assets.githubusercontent.com',
]

export function mirrorById(id: string): GithubMirror | undefined {
	return GITHUB_MIRRORS.find((mirror) => mirror.id === id)
}

/** 一个 `https://` URL 是否属于可以走镜像的 GitHub 主机。 */
export function isMirrorableUrl(url: string): boolean {
	let parsed: URL
	try {
		parsed = new URL(url)
	} catch {
		return false
	}
	if (parsed.protocol !== 'https:') return false
	return MIRRORABLE_HOSTS.includes(parsed.hostname.toLowerCase())
}

/**
 * 把 URL 改写成走镜像的 URL。主机不在白名单里、或镜像 id 不认识时，原样返回，
 * 这样调用方永远拿到一个可用的 URL，不必自己兜底。
 */
export function mirroredUrl(url: string, mirrorId: string): string {
	const mirror = mirrorById(mirrorId)
	if (!mirror || !isMirrorableUrl(url)) return url
	return `${mirror.prefix}${url}`
}

/** 镜像请求的超时。镜像挂住时不该把商店页卡住，超时后回退直连。 */
const MIRROR_TIMEOUT_MS = 20_000

export interface MirrorFetchOptions {
	enabled: boolean
	mirrorId: string
	headers?: Record<string, string>
}

/**
 * 取一个 JSON 资源，按偏好决定是否先走镜像。
 *
 * 开了镜像就先试镜像、失败再回退直连：镜像站是第三方，随时可能挂或改策略，
 * 开了镜像却因此完全用不了商店，比多花一次请求更糟。回退是静默的，结果只保证
 * 是「某一条路取到的」，不保证是镜像取的。
 */
export async function fetchJsonWithMirror<T>(
	url: string,
	options: MirrorFetchOptions,
): Promise<T> {
	const attempts: string[] = []
	if (options.enabled) {
		const mirrored = mirroredUrl(url, options.mirrorId)
		if (mirrored !== url) attempts.push(mirrored)
	}
	attempts.push(url)

	let lastError: unknown = null
	for (const attempt of attempts) {
		try {
			return await fetchJson<T>(attempt, options.headers)
		} catch (error) {
			lastError = error
		}
	}

	throw lastError instanceof Error ? lastError : new Error(String(lastError))
}

async function fetchJson<T>(
	url: string,
	headers?: Record<string, string>,
): Promise<T> {
	const controller = new AbortController()
	const timer = setTimeout(() => controller.abort(), MIRROR_TIMEOUT_MS)
	try {
		const response = await fetch(url, {
			headers: headers ?? { accept: 'application/json' },
			signal: controller.signal,
		})
		if (!response.ok) {
			throw new Error(`HTTP ${response.status}`)
		}
		// `response.json()` 对假 200（正文不是 JSON）会抛，正是我们要的：
		// 一个返回 200 但内容不对的镜像会被当成失败，进而回退直连。
		return (await response.json()) as T
	} finally {
		clearTimeout(timer)
	}
}
