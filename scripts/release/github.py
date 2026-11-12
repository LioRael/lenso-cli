"""Author/CI-only GitHub REST transport. No credential is forwarded to a download host."""
import json
import os
import urllib.error
import urllib.parse
import urllib.request

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None

def request(url, method="GET", data=None, content_type="application/json", binary=False):
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.hostname not in ("api.github.com", "uploads.github.com") or parsed.username or parsed.password:
        raise RuntimeError("Unapproved GitHub API destination")
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    headers = {"Accept": "application/octet-stream" if binary else "application/vnd.github+json",
               "User-Agent": "lenso-release", "X-GitHub-Api-Version": "2026-03-10", "Content-Type": content_type}
    if token:
        headers["Authorization"] = "Bearer " + token
    body = data if isinstance(data, bytes) or data is None else json.dumps(data).encode()
    opener = urllib.request.build_opener(NoRedirect())
    for redirect in range(5):
        try:
            with opener.open(urllib.request.Request(url, data=body, headers=headers, method=method), timeout=300) as response:
                result = response.read()
            return result if binary else json.loads(result)
        except urllib.error.HTTPError as error:
            if error.code in (301, 302, 303, 307, 308) and binary and method == "GET":
                url = urllib.parse.urljoin(url, error.headers["Location"])
                destination = urllib.parse.urlsplit(url)
                host = destination.hostname or ""
                allowed = host.endswith(".githubusercontent.com") or (host.startswith("github-production-release-asset-") and host.endswith(".amazonaws.com"))
                if destination.scheme != "https" or destination.username or destination.password or not allowed:
                    raise RuntimeError("Unapproved asset download redirect") from None
                headers.pop("Authorization", None)
                continue
            raise RuntimeError(f"GitHub {method}: HTTP {error.code}; mutations are not retried") from None
    raise RuntimeError("Asset redirect limit exceeded")

def api(repo, path, **kwargs):
    return request("https://api.github.com/repos/" + repo + "/" + path, **kwargs)
