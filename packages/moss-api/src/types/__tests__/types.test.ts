import { describe, it, expect } from "vitest";
import type {
  ProjectInfo,
  PluginManifest,
  PluginCategory,
  BaseContext,
  ProcessContext,
  GenerateContext,
  DeployContext,
  SyndicateContext,
  SourceFiles,
  ArticleInfo,
  DeploymentInfo,
  HookResult,
  PluginMessage,
  LogMessage,
  ProgressMessage,
  ErrorMessage,
  CompleteMessage,
  PageNode,
} from "../../index.js";

describe("Type Definitions", () => {
  describe("ProjectInfo", () => {
    it("accepts valid ProjectInfo object with all fields", () => {
      const info: ProjectInfo = {
        project_type: "blog",
        content_folders: ["posts", "pages"],
        total_files: 10,
        homepage_file: "index.md",
        site_name: "My Blog",
      };
      expect(info.project_type).toBe("blog");
      expect(info.content_folders).toHaveLength(2);
      expect(info.total_files).toBe(10);
      expect(info.homepage_file).toBe("index.md");
      expect(info.site_name).toBe("My Blog");
    });

    it("accepts ProjectInfo without optional homepage_file", () => {
      const info: ProjectInfo = {
        project_type: "docs",
        content_folders: [],
        total_files: 0,
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

  describe("PageNode", () => {
    it("accepts a leaf page node (markdown file)", () => {
      const node: PageNode = {
        source_path: "articles/hello.md",
        url_path: "articles/hello.html",
        title: "Hello World",
        slug: "hello",
        content_html: "<h1>Hello</h1>",
        is_folder: false,
        children: [],
        date: "2025-01-15",
        nav: false,
        draft: false,
        flatten: false,
        list_style: "list",
        also_in: [],
        frontmatter: { custom_field: "value" },
      };
      expect(node.is_folder).toBe(false);
      expect(node.children).toHaveLength(0);
      expect(node.title).toBe("Hello World");
      expect(node.content_html).toBe("<h1>Hello</h1>");
    });

    it("accepts a folder page node with children", () => {
      const child: PageNode = {
        source_path: "posts/first.md",
        url_path: "posts/first.html",
        title: "First Post",
        slug: "first",
        content_html: "<p>Content</p>",
        is_folder: false,
        children: [],
        nav: false,
        draft: false,
        flatten: false,
        list_style: "list",
        also_in: [],
        frontmatter: {},
      };

      const folder: PageNode = {
        source_path: "posts",
        url_path: "posts/index.html",
        title: "Posts",
        slug: "posts",
        content_html: "",
        is_folder: true,
        children: [child],
        nav: true,
        nav_weight: 1,
        draft: false,
        flatten: false,
        list_style: "list",
        also_in: [],
        frontmatter: {},
      };
      expect(folder.is_folder).toBe(true);
      expect(folder.children).toHaveLength(1);
      expect(folder.children[0].title).toBe("First Post");
      expect(folder.nav_weight).toBe(1);
    });

    it("accepts a folder with flatten and grid list_style", () => {
      const node: PageNode = {
        source_path: "portfolio",
        url_path: "portfolio/index.html",
        title: "Portfolio",
        slug: "portfolio",
        content_html: "<p>My work</p>",
        is_folder: true,
        children: [],
        nav: true,
        draft: false,
        flatten: true,
        list_style: "grid",
        also_in: [],
        cover: "portfolio-cover.jpg",
        frontmatter: {},
      };
      expect(node.flatten).toBe(true);
      expect(node.list_style).toBe("grid");
      expect(node.cover).toBe("portfolio-cover.jpg");
    });

    it("accepts a page with also_in cross-references", () => {
      const node: PageNode = {
        source_path: "articles/my-post.md",
        url_path: "articles/my-post.html",
        title: "My Post",
        slug: "my-post",
        content_html: "<p>Content</p>",
        is_folder: false,
        children: [],
        nav: false,
        draft: false,
        flatten: false,
        list_style: "list",
        also_in: ["weekly-highlights", "best-of-2024"],
        frontmatter: {},
      };
      expect(node.also_in).toHaveLength(2);
      expect(node.also_in).toContain("weekly-highlights");
    });

    it("accepts optional fields as undefined", () => {
      const node: PageNode = {
        source_path: "about.md",
        url_path: "about.html",
        title: "About",
        slug: "about",
        content_html: "<p>About</p>",
        is_folder: false,
        children: [],
        nav: true,
        draft: false,
        flatten: false,
        list_style: "list",
        also_in: [],
        frontmatter: {},
      };
      expect(node.date).toBeUndefined();
      expect(node.nav_weight).toBeUndefined();
      expect(node.cover).toBeUndefined();
    });
  });

  describe("GenerateContext with page_tree", () => {
    it("accepts GenerateContext with optional page_tree field", () => {
      const ctx: GenerateContext = {
        project_info: {
          project_type: "blog",
          content_folders: ["posts"],
          total_files: 5,
        },
        config: {},
        source_files: {
          markdown: ["index.md"],
          pages: [],
          docx: [],
          other: [],
        },
        page_tree: {
          source_path: "",
          url_path: "index.html",
          title: "Home",
          slug: "",
          content_html: "<p>Welcome</p>",
          is_folder: true,
          children: [],
          nav: false,
          draft: false,
          flatten: false,
          list_style: "list",
          also_in: [],
          frontmatter: {},
        },
      };
      expect(ctx.page_tree).toBeDefined();
      expect(ctx.page_tree!.is_folder).toBe(true);
      expect(ctx.page_tree!.title).toBe("Home");
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
      project_type: "blog",
      content_folders: ["posts"],
      total_files: 5,
      site_name: "My Blog",
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
      expect(ctx.project_info.project_type).toBe("blog");
      expect(ctx.trigger).toBeUndefined();
    });

    it("accepts GenerateContext with source_files", () => {
      const ctx: GenerateContext = {
        project_info: baseProjectInfo,
        config: {},
        source_files: {
          markdown: ["post1.md", "post2.md"],
          pages: ["about.md"],
          docx: [],
          other: ["image.png"],
        },
      };
      expect(ctx.source_files.markdown).toHaveLength(2);
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

  describe("SourceFiles", () => {
    it("accepts valid SourceFiles object", () => {
      const files: SourceFiles = {
        markdown: ["a.md", "b.md"],
        pages: ["about.md"],
        docx: ["doc.docx"],
        other: ["image.png", "style.css"],
      };
      expect(files.markdown).toContain("a.md");
      expect(files.docx).toContain("doc.docx");
    });

    it("accepts SourceFiles with empty arrays", () => {
      const files: SourceFiles = {
        markdown: [],
        pages: [],
        docx: [],
        other: [],
      };
      expect(files.markdown).toHaveLength(0);
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
