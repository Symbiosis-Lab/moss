import { describe, it, expect } from "vitest";
import type {
  ProjectInfo,
  PluginManifest,
  PluginCategory,
  BaseContext,
  ProcessContext,
  DeployContext,
  SyndicateContext,
  ArticleInfo,
  DeploymentInfo,
  HookResult,
  PluginMessage,
  LogMessage,
  ProgressMessage,
  ErrorMessage,
  CompleteMessage,
} from "../../index.js";

describe("Type Definitions", () => {
  describe("ProjectInfo", () => {
    it("accepts valid ProjectInfo object with all fields", () => {
      const info: ProjectInfo = {
        total_files: 10,
        homepage_file: "index.md",
        folder_name: "My Blog",
        site_name: "My Blog",
        lang: "en",
      };
      expect(info.total_files).toBe(10);
      expect(info.homepage_file).toBe("index.md");
      expect(info.site_name).toBe("My Blog");
    });

    it("accepts ProjectInfo without optional homepage_file", () => {
      const info: ProjectInfo = {
        total_files: 0,
        lang: "en",
      };
      expect(info.homepage_file).toBeUndefined();
    });
  });

  describe("PluginManifest", () => {
    it("accepts valid PluginManifest with required fields only", () => {
      const manifest: PluginManifest = {
        name: "my-plugin",
        version: "1.0.0",
        entry: "main.js",
        category: "deployer",
      };
      expect(manifest.name).toBe("my-plugin");
      expect(manifest.category).toBe("deployer");
    });

    it("accepts PluginManifest with all optional fields", () => {
      const manifest: PluginManifest = {
        name: "full-plugin",
        version: "2.0.0",
        entry: "main.bundle.js",
        category: "syndicator",
        global_name: "FullPlugin",
        icon: "icon.svg",
        domain: "example.com",
        config: { key: "value", nested: { deep: true } },
      };
      expect(manifest.global_name).toBe("FullPlugin");
      expect(manifest.icon).toBe("icon.svg");
      expect(manifest.domain).toBe("example.com");
      expect(manifest.config).toEqual({ key: "value", nested: { deep: true } });
    });
  });

  describe("PluginCategory", () => {
    it("accepts all valid category values", () => {
      const categories: PluginCategory[] = [
        "generator",
        "deployer",
        "syndicator",
        "enhancer",
        "processor",
      ];
      expect(categories).toHaveLength(5);
      expect(categories).toContain("generator");
      expect(categories).toContain("deployer");
      expect(categories).toContain("syndicator");
      expect(categories).toContain("enhancer");
      expect(categories).toContain("processor");
    });
  });

  describe("Context Types", () => {
    const baseProjectInfo: ProjectInfo = {
      total_files: 5,
      site_name: "My Blog",
      lang: "en",
    };

    it("accepts valid BaseContext", () => {
      const ctx: BaseContext = {
        project_info: baseProjectInfo,
        config: { setting: true },
      };
      expect(ctx.project_info).toBe(baseProjectInfo);
      expect(ctx.config).toEqual({ setting: true });
    });

    it("accepts ProcessContext (extends BaseContext)", () => {
      const ctx: ProcessContext = {
        project_info: baseProjectInfo,
        config: {},
      };
      expect(ctx.project_info.lang).toBe("en");
      expect(ctx.trigger).toBeUndefined();
    });

    it("accepts DeployContext with site_files", () => {
      const ctx: DeployContext = {
        project_info: baseProjectInfo,
        config: {},
        site_files: ["index.html", "post1.html"],
      };
      expect(ctx.site_files).toHaveLength(2);
    });

    it("accepts DeployContext with optional domain", () => {
      const ctxWithDomain: DeployContext = {
        project_info: baseProjectInfo,
        config: {},
        site_files: ["index.html"],
        domain: "example.com",
      };
      expect(ctxWithDomain.domain).toBe("example.com");

      const ctxWithoutDomain: DeployContext = {
        project_info: baseProjectInfo,
        config: {},
        site_files: ["index.html"],
      };
      expect(ctxWithoutDomain.domain).toBeUndefined();
    });

    it("accepts SyndicateContext with articles and optional deployment", () => {
      const ctx: SyndicateContext = {
        project_info: baseProjectInfo,
        config: {},
        site_files: ["index.html"],
        articles: [
          {
            source_path: "/project/posts/hello.md",
            title: "Hello World",
            content: "# Hello\n\nContent here",
            frontmatter: { draft: false },
            url_path: "/posts/hello.html",
            date: "2024-01-01",
            tags: ["intro", "blog"],
          },
        ],
        deployment: {
          method: "github-pages",
          url: "https://example.github.io",
          deployed_at: "2024-01-01T12:00:00Z",
          metadata: { branch: "gh-pages" },
        },
      };
      expect(ctx.articles).toHaveLength(1);
      expect(ctx.articles[0].title).toBe("Hello World");
      expect(ctx.deployment?.method).toBe("github-pages");
    });
  });

  describe("ArticleInfo", () => {
    it("accepts ArticleInfo with all fields", () => {
      const article: ArticleInfo = {
        source_path: "/posts/hello.md",
        title: "Hello World",
        content: "# Hello",
        frontmatter: { tags: ["test"] },
        url_path: "/posts/hello.html",
        date: "2024-01-01",
        tags: ["test"],
      };
      expect(article.title).toBe("Hello World");
      expect(article.date).toBe("2024-01-01");
    });

    it("accepts ArticleInfo without optional date", () => {
      const article: ArticleInfo = {
        source_path: "/pages/about.md",
        title: "About",
        content: "About us",
        frontmatter: {},
        url_path: "/about.html",
        tags: [],
      };
      expect(article.date).toBeUndefined();
    });
  });

  describe("DeploymentInfo", () => {
    it("accepts valid DeploymentInfo", () => {
      const deployment: DeploymentInfo = {
        method: "github-pages",
        url: "https://example.github.io/repo",
        deployed_at: "2024-01-01T12:00:00Z",
        metadata: {
          branch: "gh-pages",
          commit: "abc123",
        },
      };
      expect(deployment.method).toBe("github-pages");
      expect(deployment.metadata.branch).toBe("gh-pages");
    });
  });

  describe("HookResult", () => {
    it("accepts success result with minimal fields", () => {
      const result: HookResult = {
        success: true,
      };
      expect(result.success).toBe(true);
      expect(result.message).toBeUndefined();
    });

    it("accepts success result with message", () => {
      const result: HookResult = {
        success: true,
        message: "Deployed successfully",
      };
      expect(result.message).toBe("Deployed successfully");
    });

    it("accepts failure result with deployment info", () => {
      const result: HookResult = {
        success: false,
        message: "Partial deployment",
        deployment: {
          method: "netlify",
          url: "https://example.netlify.app",
          deployed_at: "2024-01-01T12:00:00Z",
          metadata: {},
        },
      };
      expect(result.success).toBe(false);
      expect(result.deployment?.url).toBe("https://example.netlify.app");
    });
  });

  describe("PluginMessage discriminated union", () => {
    it("correctly discriminates LogMessage", () => {
      const msg: PluginMessage = {
        type: "log",
        level: "warn",
        message: "Warning text",
      };
      expect(msg.type).toBe("log");
      if (msg.type === "log") {
        expect(msg.level).toBe("warn");
        expect(msg.message).toBe("Warning text");
      }
    });

    it("correctly discriminates ProgressMessage", () => {
      const msg: PluginMessage = {
        type: "progress",
        phase: "building",
        current: 5,
        total: 10,
        message: "Half done",
      };
      expect(msg.type).toBe("progress");
      if (msg.type === "progress") {
        expect(msg.phase).toBe("building");
        expect(msg.current).toBe(5);
        expect(msg.total).toBe(10);
      }
    });

    it("correctly discriminates ErrorMessage", () => {
      const msg: PluginMessage = {
        type: "error",
        error: "Something broke",
        context: "deployment",
        fatal: true,
      };
      expect(msg.type).toBe("error");
      if (msg.type === "error") {
        expect(msg.error).toBe("Something broke");
        expect(msg.fatal).toBe(true);
      }
    });

    it("correctly discriminates CompleteMessage", () => {
      const msg: PluginMessage = {
        type: "complete",
        success: true,
        result: { data: "done" },
      };
      expect(msg.type).toBe("complete");
      if (msg.type === "complete") {
        expect(msg.result).toEqual({ data: "done" });
      }
    });

    it("LogMessage accepts all log levels", () => {
      const levels: Array<"log" | "warn" | "error"> = ["log", "warn", "error"];
      levels.forEach((level) => {
        const msg: LogMessage = {
          type: "log",
          level,
          message: "test",
        };
        expect(msg.level).toBe(level);
      });
    });

    it("ProgressMessage works without optional message", () => {
      const msg: ProgressMessage = {
        type: "progress",
        phase: "scanning",
        current: 0,
        total: 100,
      };
      expect(msg.message).toBeUndefined();
    });

    it("ErrorMessage works without optional context", () => {
      const msg: ErrorMessage = {
        type: "error",
        error: "Failed",
        fatal: false,
      };
      expect(msg.context).toBeUndefined();
    });

    it("CompleteMessage accepts any result type", () => {
      const messages: CompleteMessage[] = [
        { type: "complete", success: true, result: null },
        { type: "complete", success: true, result: undefined },
        { type: "complete", success: true, result: 42 },
        { type: "complete", success: true, result: "string" },
        { type: "complete", success: true, result: { nested: { deep: true } } },
        { type: "complete", success: true, result: [1, 2, 3] },
      ];
      expect(messages).toHaveLength(6);
    });
  });
});
