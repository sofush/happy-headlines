workspace "HappyHeadlines System" {

    description "C4 Model (Context, Container, and Component levels) for the HappyHeadlines positive news platform."

    model {
        !identifiers hierarchical

        # --- People / Users ---
        publisher = person "Publisher" "Writes, drafts, and publishes news articles." {
            tag "Person"
        }
        
        reader = person "Reader" "Reads articles, posts comments, and subscribes to daily newsletters." {
            tag "Person"
        }

        # --- Main Software System ---
        happyHeadlines = softwareSystem "HappyHeadlines System" "Allows publishers to write articles and readers to read positive news, comment, and subscribe to newsletters." {
            
            # --- Containers Inside HappyHeadlines ---
            
            # User Interfaces
            webapp = container "Webapp" "Allows publishers to draft and publish articles." "Web Application" {
                tag "Web App"
            }
            
            website = container "Website" "Displays articles, allows readers to post comments, and handles newsletter subscriptions." "Web Application" {
                tag "Web App"
            }

            # Services
            draftService = container "DraftService" "Manages saving and retrieving article drafts." "Service" {
                tag "Service"
            }
            
            publisherService = container "PublisherService" "Finalises publication requests, checks content via ProfanityService, and queues articles." "Service" {
                tag "Service"
            }
            
            profanityService = container "ProfanityService" "Filters out inappropriate language using the ProfanityDatabase." "Service" {
                tag "Service"
            }
            
            articleService = container "ArticleService" "Retrieves article content from database and subscribes to published articles in the queue." "Service" {
                tag "Service"

                # Level 3 components of ArticleService
                articleRouter = component "Article Router" "Router that maps HTTP requests to controllers and configures middleware." {
                    tag "Component"
                }

                articleController = component "Article Controller" "HTTP handlers that expose the article CRUD endpoints and fetches data from the repository." {
                    tag "Component"
                }

                articleRepository = component "Article Repository" "Checks Redis first, then reads from and writes to the PostgreSQL database/shard." {
                    tag "Component"
                }

                articleCache = component "Article Cache" "Reads, writes, and evicts cached article in Redis." {
                    tag "Component"
                }

                telemetry = component "Telemetry" "Configures OpenTelemetry tracing and Prometheus metrics." {
                    tag "Component"
                }

                articleRouter -> articleController "Routes article requests to"
                articleRouter -> telemetry "Configures request tracing and metrics through"
                articleController -> articleRepository "Delegates article operations to"
                articleRepository -> articleCache "Checks for and stores cached articles in"
                articleController -> telemetry "Emits request traces through"
                articleRepository -> telemetry "Emits data access traces through"
                articleCache -> telemetry "Emits cache traces through"
            }
            
            commentService = container "CommentService" "Handles posting, filtering, and retrieval of reader comments." "Service" {
                tag "Service"
            }
            
            subscriberService = container "SubscriberService" "Manages subscriber requests and information, and places new subscribers in the subscriber queue." "Service" {
                tag "Service"
            }
            
            newsletterService = container "NewsletterService" "Manages the daily newsletter process, sends updates to active subscribers, and sends welcome emails to new subscribers." "Service" {
                tag "Service"
            }

            # Message Queues
            articleQueue = container "ArticleQueue" "Holds approved articles waiting to be persisted and consumed." "Message Queue" {
                tag "Queue"
            }
            
            subscriberQueue = container "SubscriberQueue" "Holds new subscriber requests for decoupled processing." "Message Queue" {
                tag "Queue"
            }

            # Databases / Data Stores
            draftDatabase = container "DraftDatabase" "Stores draft articles." "Database" {
                tag "Database"
            }
            
            articleDatabase = container "ArticleDatabase" "Stores published articles." "Database" {
                tag "Database"
            }
            
            articleCacheStore = container "ArticleCacheStore" "Stores cached versions of recently published articles." "Redis" {
                tag "Cache"
            }
            
            commentDatabase = container "CommentDatabase" "Stores reader comments." "Database" {
                tag "Database"
            }
            
            profanityDatabase = container "ProfanityDatabase" "Stores prohibited words for filtering." "Database" {
                tag "Database"
            }
            
            subscriberDatabase = container "SubscriberDatabase" "Stores subscriber information." "Database" {
                tag "Database"
            }

            # --- Container Level Relationships ---
            
            # Publisher interactions
            publisher -> webapp "Writes, drafts, and publishes articles using"
            webapp -> draftService "Saves and retrieves drafts using"
            webapp -> publisherService "Sends ready articles for publication to"
            draftService -> draftDatabase "Stores and retrieves drafts from"
            
            publisherService -> profanityService "Consults for profanity filtering before publishing"
            publisherService -> articleQueue "Places approved articles into"
            articleQueue -> articleDatabase "Persists queued articles into"
            
            # Reader interactions & Reading
            reader -> website "Reads articles, posts comments, and subscribes via"
            website -> articleService "Fetches article data from"
            articleService -> articleDatabase "Retrieves article content from"
            articleService -> articleCacheStore "Caches and retrieves articles from"
            articleService -> articleQueue "Subscribes to new articles in"
            
            # Reader commenting
            website -> commentService "Passes posted comments to"
            commentService -> profanityService "Filters profanity using"
            commentService -> commentDatabase "Stores and retrieves comments through"
            
            # Profanity Service shared DB
            profanityService -> profanityDatabase "Retrieves and manages prohibited words from"
            
            # Reader subscriptions & Newsletter
            website -> subscriberService "Sends subscription requests to"
            subscriberService -> subscriberDatabase "Stores subscriber info in"
            subscriberService -> subscriberQueue "Places new subscribers into"
            
            newsletterService -> articleService "Retrieves recent articles from"
            newsletterService -> subscriberService "Retrieves active subscriber info and new subscribers from"
            newsletterService -> reader "Sends daily newsletter emails and welcome emails to new subscribers"
        }

        # --- Component Level Relationships ---
        happyHeadlines.articleService.articleRepository -> happyHeadlines.articleDatabase "Reads and writes article rows in"
        happyHeadlines.articleService.articleCache -> happyHeadlines.articleCacheStore "Caches article in"
    }

    views {
        systemContext happyHeadlines "SystemContext" {
            include *
            autoLayout lr
            description "Context diagram for the HappyHeadlines system."
        }

        container happyHeadlines "Containers" {
            include *
            autoLayout lr
            description "Container diagram showing services, databases, queues, and user interfaces in HappyHeadlines."
        }

        component happyHeadlines.articleService "ArticleServiceComponents" {
            include *
            autoLayout lr
            description "Level 3 component diagram for ArticleService showing the router, controller, repository, cache, and telemetry components."
        }

        styles {
            element "Person" {
                shape Person
                background #08427b
                color #ffffff
            }
            element "Software System" {
                background #1168bd
                color #ffffff
            }
            element "Web App" {
                shape WebBrowser
                background #438dd5
                color #ffffff
            }
            element "Service" {
                shape Hexagon
                background #85bbf0
                color #000000
            }
            element "Database" {
                shape Cylinder
                background #999999
                color #ffffff
            }
            element "Cache" {
                shape Cylinder
                background #cc3333
                color #ffffff
            }
            element "Component" {
                shape RoundedBox
                background #b3d4f5
                color #000000
            }
            element "Queue" {
                shape Pipe
                background #cccccc
                color #000000
            }
        }
    }
}
